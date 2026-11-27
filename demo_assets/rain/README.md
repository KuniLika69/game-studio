# Мастерская после дождя · ресурсы

Геометрия мостовой, гранитного бордюра, фасада, пола, мебели и инструментов сделана для этой сцены. Скрипт `build.py` создаёт GLB и встраивает карты цвета, нормалей и шероховатости из `scans/`. Бордюр и верстак используют процедурные карты.

- Мостовая — [Cobblestone Pavement, Charlotte Baglioni](https://polyhaven.com/a/cobblestone_pavement).
- Пол — [Wood Floor Worn, Dimitrios Savva](https://polyhaven.com/a/wood_floor_worn).
- Кирпичный фасад — [Brick Wall 09, Matterfield и Rob Tuytel](https://polyhaven.com/a/brick_wall_09).
- Стены мастерской и каменные наличники — [Plaster Stone Wall 02, Charlotte Baglioni](https://polyhaven.com/a/plaster_stone_wall_02).
- Почва дельты и речная галька — [Forest Ground 06](https://polyhaven.com/a/forest_ground_06) и [Dry River Pebbles](https://polyhaven.com/a/dry_river_pebbles). Рельеф, камни, деревья и камыш генерируются в `../expedition/generate.py`.

Сканы предоставлены Poly Haven под [CC0](https://polyhaven.com/license). Исходники в `scans/` сохранены в 1K–2K. В сценах используются карты 512–1024 пикселя с повтором в масштабе материала. Надпись оформлена системным Bahnschrift.

Арсен и Лида собраны на изменяемых основах Райана и Эммы. Скелеты, текстуры кожи и исходные клипы опираются на Microsoft Rocketbox. Записанная речь Лиды создана локальным русским синтезатором Microsoft Irina Desktop. Файл WAV и события артикуляции входят в сборку. Для камня, дерева и воды добавлены по три записи отдельных шагов. [Модели и лицензии](../README.md) · [Голос и мимика](../workshop_audio/README.md) · [Записи шагов](../workshop_audio/steps/README.md)

Оригинальные GLB можно заменить через инспектор. Точечный свет, коллайдер и игровые правила хранятся отдельно от внешнего вида. Мокрый камень отражает небосвод и локальные источники. Мелкая вода переиспользует приближённые силуэты фасадов. Точечные светильники освещают материалы, а направленные тени и контактная маска обрабатываются отдельно.
