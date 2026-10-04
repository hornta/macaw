//! A small model of Windows for tests: it applies the engine's decisions in the order Windows
//! would (the forwarded original first, then injected input) and records what Windows receives.

use std::collections::BTreeSet;

use crate::{Action, Config, Ctx, Decision, Engine, Input, Key, Out, Typematic};

pub struct Sim {
    pub engine: Engine,
    pub ctx: Ctx,
    /// Milliseconds; every key event advances it by 10.
    pub now: u64,
    /// Keys Windows currently considers down.
    pub down: BTreeSet<Key>,
    /// What Windows received, as tokens: `+LCtrl`, `-C`, `mask`, `'é'`, `Lock`, `paused`.
    pub trace: Vec<String>,
    pub typed: String,
    pub actions: Vec<Action>,
    /// Key-ups Windows received for keys it didn't consider down.
    pub stray_ups: usize,
}

impl Sim {
    pub fn new(config: Config) -> Sim {
        let mut engine = Engine::new(config);
        engine.set_typematic(Typematic {
            delay_ms: 250,
            interval_ms: 33,
            repeats: true,
        });
        Sim {
            engine,
            ctx: Ctx {
                remap: true,
                ..Ctx::default()
            },
            now: 1_000,
            down: BTreeSet::new(),
            trace: Vec::new(),
            typed: String::new(),
            actions: Vec::new(),
            stray_ups: 0,
        }
    }

    pub fn press(&mut self, key: Key) -> &mut Sim {
        self.event(key, true)
    }

    pub fn release(&mut self, key: Key) -> &mut Sim {
        self.event(key, false)
    }

    pub fn tap(&mut self, key: Key) -> &mut Sim {
        self.press(key).release(key)
    }

    /// Presses the modifiers in order, taps `key`, then releases the modifiers in reverse.
    pub fn chord(&mut self, mods: &[Key], key: Key) -> &mut Sim {
        for &m in mods {
            self.press(m);
        }
        self.tap(key);
        for &m in mods.iter().rev() {
            self.release(m);
        }
        self
    }

    pub fn event(&mut self, key: Key, down: bool) -> &mut Sim {
        self.now += 10;
        let decision = self.engine.process(
            Input {
                key,
                down,
                time: self.now,
            },
            &self.ctx,
        );
        self.apply(key, down, decision);
        self
    }

    /// Replays a recorded event: lets time pass until `time` (running the guard on the way),
    /// then processes the event.
    pub fn replay(&mut self, time: u64, key: Key, down: bool) -> &mut Sim {
        if time > self.now {
            self.wait(time - self.now);
        }
        let decision = self.engine.process(
            Input {
                key,
                down,
                time: self.now,
            },
            &self.ctx,
        );
        self.apply(key, down, decision);
        self
    }

    /// Lets time pass, running the stuck-key guard every 50 ms like the backend does.
    pub fn wait(&mut self, ms: u64) -> &mut Sim {
        let end = self.now + ms;
        while self.now < end {
            self.now = (self.now + 50).min(end);
            let out = self.engine.tick(self.now);
            self.apply_out(out);
        }
        self
    }

    /// Like the backend on lock, unlock and resume: release everything the engine holds.
    pub fn reset(&mut self) -> &mut Sim {
        let out = self.engine.reset();
        self.apply_out(out);
        self
    }

    /// The trace so far, space separated; clears it.
    pub fn take(&mut self) -> String {
        let text = self.trace.join(" ");
        self.trace.clear();
        text
    }

    fn apply(&mut self, key: Key, down: bool, decision: Decision) {
        if decision.forward {
            self.key(key, down);
        }
        self.apply_out(decision.out);
    }

    fn apply_out(&mut self, out: Vec<Out>) {
        for o in out {
            match o {
                Out::Key { key, down } => self.key(key, down),
                Out::Mask => self.trace.push("mask".into()),
                Out::Text(c) => {
                    self.typed.push(c);
                    self.trace.push(format!("'{c}'"));
                }
                Out::Action(action) => {
                    self.actions.push(action);
                    self.trace.push(format!("{action:?}"));
                }
                Out::Paused(paused) => self.trace.push(if paused { "paused" } else { "resumed" }.into()),
            }
        }
    }

    fn key(&mut self, key: Key, down: bool) {
        if down {
            self.down.insert(key);
        } else if !self.down.remove(&key) {
            self.stray_ups += 1;
        }
        self.trace.push(format!("{}{key:?}", if down { '+' } else { '-' }));
    }
}
