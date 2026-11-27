use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread::{self, JoinHandle},
};

static VIDEO_SEQUENCE: AtomicU64 = AtomicU64::new(1);
const MAX_ERROR_BYTES: usize = 4096;

/// Records RGB frames from the application render target. No window or screen input is used.
pub struct Video {
    child: Option<Child>,
    input: Option<ChildStdin>,
    errors: Option<JoinHandle<String>>,
    temporary: TemporaryVideo,
    output: PathBuf,
    frame_bytes: usize,
    frame_count: u64,
}

impl Video {
    pub fn start(output: &Path, width: u32, height: u32, fps: u32) -> Result<Self, String> {
        if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 || fps == 0 {
            return Err("Для MP4 нужны ненулевая частота и чётные размеры кадра.".into());
        }
        let frame_bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(3))
            .ok_or("Разрешение видео слишком велико.")?;
        let name = output.file_name().ok_or("Укажи имя файла видео.")?;
        let dir = output
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .canonicalize()
            .map_err(|error| format!("Не удалось открыть папку видео ({error})."))?;
        let output = dir.join(name);
        match fs::symlink_metadata(&output) {
            Ok(_) => return Err("Файл видео с таким именем уже существует.".into()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Не удалось проверить путь видео ({error}).")),
        }
        let sequence = VIDEO_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let staging_path = dir.join(format!(
            ".studio-proof-{}-{sequence}.mp4",
            std::process::id()
        ));
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging_path)
            .map_err(|error| format!("Не удалось создать временный файл видео ({error})."))?;
        let temporary = TemporaryVideo(staging_path);

        let mut command = Command::new("ffmpeg");
        command
            .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y"])
            .args(["-f", "rawvideo", "-pixel_format", "rgb24"])
            .args(["-video_size", &format!("{width}x{height}")])
            .args(["-framerate", &fps.to_string(), "-i", "pipe:0"])
            .args([
                "-an", "-c:v", "libx264", "-preset", "veryfast", "-crf", "18",
            ])
            .args([
                "-pix_fmt",
                "yuv420p",
                "-movflags",
                "+faststart",
                "-f",
                "mp4",
            ])
            .arg(&temporary.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("Не удалось запустить FFmpeg ({error})."))?;
        let pipe = child.stderr.take().expect("FFmpeg stderr was piped");
        let errors = match thread::Builder::new()
            .name("video-errors".into())
            .spawn(move || read_errors(pipe))
        {
            Ok(errors) => errors,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Не удалось открыть журнал кодировщика ({error})."));
            }
        };

        Ok(Self {
            input: child.stdin.take(),
            child: Some(child),
            errors: Some(errors),
            temporary,
            output,
            frame_bytes,
            frame_count: 0,
        })
    }

    pub fn push(&mut self, rgb24: &[u8], repeats: u32) -> Result<(), String> {
        if rgb24.len() != self.frame_bytes {
            return Err(format!(
                "В кадре RGB24 {} байт, ожидалось {}.",
                rgb24.len(),
                self.frame_bytes,
            ));
        }
        let input = self.input.as_mut().ok_or("Запись видео уже остановлена.")?;
        for _ in 0..repeats {
            if let Err(error) = input.write_all(rgb24) {
                self.abort();
                return Err(self.error(format!("Не удалось записать кадр ({error}).")));
            }
            self.frame_count += 1;
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<(), String> {
        if self.frame_count == 0 {
            return Err("Видео не содержит кадров.".into());
        }
        self.input.take();
        let child = self.child.as_mut().ok_or("Кодировщик остановлен.")?;
        let status = child
            .wait()
            .map_err(|error| format!("Не удалось завершить кодирование видео ({error})."))?;
        self.child.take();
        let message = self.error(format!("FFmpeg завершился с кодом {status}."));
        if !status.success() {
            return Err(message);
        }
        let size = fs::metadata(&self.temporary.0)
            .map_err(|error| format!("Не удалось открыть созданное видео ({error})."))?
            .len();
        if size == 0 {
            return Err("FFmpeg создал пустой файл видео.".into());
        }
        // Linking from the same directory commits the completed file without replacing one
        // that appeared while FFmpeg was writing. Drop removes the temporary link.
        fs::hard_link(&self.temporary.0, &self.output)
            .map_err(|error| format!("Не удалось сохранить готовый MP4 ({error})."))
    }

    fn abort(&mut self) {
        self.input.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn error(&mut self, mut message: String) -> String {
        if let Some(log) = self.errors.take().and_then(|reader| reader.join().ok())
            && !log.is_empty()
        {
            message.push(' ');
            message.push_str(&log);
        }
        message
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        self.abort();
        if let Some(errors) = self.errors.take() {
            let _ = errors.join();
        }
    }
}

struct TemporaryVideo(PathBuf);

impl Drop for TemporaryVideo {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn read_errors(mut pipe: impl Read) -> String {
    let mut tail = Vec::with_capacity(MAX_ERROR_BYTES);
    let mut buffer = [0; 1024];
    while let Ok(bytes) = pipe.read(&mut buffer) {
        if bytes == 0 {
            break;
        }
        let excess = (tail.len() + bytes).saturating_sub(MAX_ERROR_BYTES);
        tail.drain(..excess);
        tail.extend_from_slice(&buffer[..bytes]);
    }
    String::from_utf8_lossy(&tail).trim().to_owned()
}
