/// Execution order for already matched rules. Scene data is supplied by a host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome<Condition, Command> {
    Fired { actions: usize },
    Blocked(Condition),
    AlreadyFired,
    MissingActionTarget(Command),
}

pub struct MatchedRule<'a, Condition, Command> {
    pub id: u32,
    pub once: bool,
    pub conditions: &'a [Condition],
    pub actions: &'a [Command],
}

pub trait Host {
    type Condition: Copy;
    type Command: Copy;

    fn holds(&self, condition: Self::Condition) -> bool;
    fn supports(&self, command: Self::Command) -> bool;
    fn already_fired(&self, id: u32) -> bool;
    fn apply(&mut self, command: Self::Command);
    fn remember_once(&mut self, id: u32);
    fn record(&mut self, id: u32, outcome: Outcome<Self::Condition, Self::Command>);
    fn halted(&self) -> bool;
}

pub fn dispatch<'a, H: Host>(
    host: &mut H,
    rules: impl Iterator<Item = MatchedRule<'a, H::Condition, H::Command>>,
) -> usize
where
    H::Condition: 'a,
    H::Command: 'a,
{
    let mut fired = 0;
    for rule in rules {
        if host.halted() {
            break;
        }
        let outcome =
            if let Some(&condition) = rule.conditions.iter().find(|&&item| !host.holds(item)) {
                Outcome::Blocked(condition)
            } else if rule.once && host.already_fired(rule.id) {
                Outcome::AlreadyFired
            } else if let Some(&action) = rule.actions.iter().find(|&&item| !host.supports(item)) {
                Outcome::MissingActionTarget(action)
            } else {
                for &action in rule.actions {
                    host.apply(action);
                }
                if rule.once {
                    host.remember_once(rule.id);
                }
                fired += 1;
                Outcome::Fired {
                    actions: rule.actions.len(),
                }
            };
        host.record(rule.id, outcome);
    }
    fired
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Command {
        Add,
        Missing,
        Stop,
    }

    #[derive(Default)]
    struct State {
        value: i32,
        once: Vec<u32>,
        stopped: bool,
        journal: Vec<(u32, Outcome<i32, Command>)>,
    }

    impl Host for State {
        type Condition = i32;
        type Command = Command;

        fn holds(&self, threshold: i32) -> bool {
            self.value >= threshold
        }
        fn supports(&self, command: Command) -> bool {
            command != Command::Missing
        }
        fn already_fired(&self, id: u32) -> bool {
            self.once.contains(&id)
        }
        fn apply(&mut self, command: Command) {
            match command {
                Command::Add => self.value += 1,
                Command::Missing => unreachable!(),
                Command::Stop => self.stopped = true,
            }
        }
        fn remember_once(&mut self, id: u32) {
            if !self.stopped {
                self.once.push(id);
            }
        }
        fn record(&mut self, id: u32, outcome: Outcome<i32, Command>) {
            self.journal.push((id, outcome));
        }
        fn halted(&self) -> bool {
            self.stopped
        }
    }

    #[test]
    fn matched_rules_execute_in_order_and_validate_every_command_before_effects() {
        let mut state = State::default();
        let rule = |id, once, conditions, actions| MatchedRule {
            id,
            once,
            conditions,
            actions,
        };
        let first = || rule(1, true, &[], &[Command::Add]);
        assert_eq!(dispatch(&mut state, [first(), first()].into_iter()), 1);
        assert_eq!(state.value, 1);
        assert_eq!(state.journal[1], (1, Outcome::AlreadyFired));
        assert_eq!(
            dispatch(
                &mut state,
                [
                    rule(2, false, &[2], &[Command::Add]),
                    rule(3, false, &[1], &[Command::Add, Command::Missing]),
                    rule(4, true, &[1], &[Command::Stop]),
                    rule(5, false, &[], &[Command::Add]),
                ]
                .into_iter()
            ),
            1
        );
        assert_eq!(state.value, 1);
        assert_eq!(state.once, [1]);
        assert_eq!(
            state.journal,
            [
                (1, Outcome::Fired { actions: 1 }),
                (1, Outcome::AlreadyFired),
                (2, Outcome::Blocked(2)),
                (3, Outcome::MissingActionTarget(Command::Missing)),
                (4, Outcome::Fired { actions: 1 }),
            ]
        );
    }
}
