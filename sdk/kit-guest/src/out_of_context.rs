//! What a kit does with its elements OUT of their contexts (docs/hors-contexte.md): game.engine's stack of contexts
//! (docs/contextes.md) says which context is on top; an element of a kit (an animated entity, the physics world, a
//! voice) runs in some contexts, and when the top leaves them it FREEZES (held where it is, takes up where it was),
//! STOPS (held and set back to its start: it starts again when its context comes back), CONTINUES, or ENDS (gone for
//! good, where a kit has such a thing: a voice). The behaviour is declared per kit (config `outOfContext`: a
//! behaviour, the contexts, a behaviour per kind of element), per element (its own `contexts` and `outOfContext`, in
//! data or by the kit's `setOutOfContext`), and changed by the kit's hook `<kit>.context.leave` (behaviour changeable,
//! cancel = continue), asked only when a tap listens.
//!
//! This file is the pure rule and the two calls every kit needs (follow the stack, ask the hook); what a behaviour
//! does to an element is the kit's own (a playback rate, the stepping of the physics, a voice paused). Cost: nothing
//! without game.engine (no subscription); with it, one event per change of the stack, never per tick nor per frame,
//! and a kit only looks at an element's flag (`applied`) at its tick. Used by: core.animation, core.physics,
//! core.audio (their `logic/context_out.rs`).

use crate::host::Kx;
use serde_json::{json, Value as Json};
use std::collections::BTreeMap;

/// game.engine's event of every change of its stack (`{ top, from, stack, declared, op }`).
pub const CONTEXT_EVENT: &str = "game.engine.context.changed";
/// The context at the bottom of the stack, the one of an element that names none.
pub const DEFAULT: &str = "default";
/// The value of a declaration that hears whatever is on top (never a context of the stack).
pub const ALL: &str = "all";

/// What an element does while the context on top is not one of its contexts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Behavior {
    /// Held where it is; takes up where it was when its context comes back.
    Freeze,
    /// Held and set back to its start; starts again when its context comes back.
    Stop,
    /// Goes on as if nothing changed.
    Continue,
    /// Ends for good (a kit without such a thing reads it as `Stop`).
    End,
}

impl Behavior {
    /// `freeze`, `stop`, `continue`, `end`; anything else is none.
    pub fn parse(s: &str) -> Option<Behavior> {
        Some(match s {
            "freeze" => Behavior::Freeze,
            "stop" => Behavior::Stop,
            "continue" => Behavior::Continue,
            "end" => Behavior::End,
            _ => return None,
        })
    }
    /// The behaviour a JSON value names (a string), else none.
    pub fn of(v: &Json) -> Option<Behavior> {
        v.as_str().and_then(Behavior::parse)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Behavior::Freeze => "freeze",
            Behavior::Stop => "stop",
            Behavior::Continue => "continue",
            Behavior::End => "end",
        }
    }
}

/// The contexts a value names: a name, a list of names, or nothing (empty: the default of whoever reads it).
pub fn contexts_of(v: &Json) -> Vec<String> {
    match v {
        Json::String(s) if !s.is_empty() => vec![s.clone()],
        Json::Array(a) => a
            .iter()
            .filter_map(Json::as_str)
            .filter(|s| !s.is_empty())
            .take(64)
            .map(String::from)
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether contexts `contexts` hear the context `top` (empty: `default`; `all` hears every one; `*` is read as `all`).
pub fn hears(contexts: &[String], top: &str) -> bool {
    if contexts.is_empty() {
        return top == DEFAULT;
    }
    contexts.iter().any(|c| c == top || c == ALL || c == "*")
}

/// A kit's rule: the behaviour of its elements, the contexts of those that name none, a behaviour per kind.
#[derive(Clone, Debug, PartialEq)]
pub struct Policy {
    pub behavior: Behavior,
    /// The contexts of an element that names none (empty: `default`, or what the kit decides, as core.audio's voices
    /// that run in the context they were played in).
    pub contexts: Vec<String>,
    /// A behaviour per kind of element (core.audio: per bus), over `behavior`.
    pub kinds: BTreeMap<String, Behavior>,
}

impl Policy {
    /// The rule of a kit whose elements do `behavior` by default.
    pub fn new(behavior: Behavior) -> Policy {
        Policy {
            behavior,
            contexts: Vec::new(),
            kinds: BTreeMap::new(),
        }
    }
    /// The rule `v` over this one: a behaviour (`"freeze"`), or `{ behavior, contexts, kinds: { kind: behaviour } }`
    /// (each field optional; a kind given `null` goes back to `behavior`). An unknown behaviour is refused.
    pub fn apply(&mut self, v: &Json) -> Result<(), String> {
        match v {
            Json::Null => Ok(()),
            Json::String(s) => {
                self.behavior = Behavior::parse(s).ok_or_else(|| unknown(s))?;
                Ok(())
            }
            Json::Object(o) => {
                if let Some(b) = o.get("behavior").filter(|b| !b.is_null()) {
                    self.behavior = Behavior::of(b).ok_or_else(|| unknown(&b.to_string()))?;
                }
                if let Some(c) = o.get("contexts") {
                    self.contexts = contexts_of(c);
                }
                for (kind, b) in o
                    .get("kinds")
                    .and_then(Json::as_object)
                    .into_iter()
                    .flatten()
                {
                    if b.is_null() {
                        self.kinds.remove(kind);
                    } else {
                        let b = Behavior::of(b).ok_or_else(|| unknown(&b.to_string()))?;
                        self.kinds.insert(kind.clone(), b);
                    }
                }
                Ok(())
            }
            _ => Err("outOfContext: a behaviour or { behavior, contexts, kinds }".into()),
        }
    }
    /// The behaviour of an element of kind `kind` that asks `own` (none: its kind's, else the kit's).
    pub fn behavior_for(&self, own: Option<Behavior>, kind: &str) -> Behavior {
        own.or_else(|| self.kinds.get(kind).copied())
            .unwrap_or(self.behavior)
    }
    /// What `outOfContext()` answers: the rule as data.
    pub fn to_json(&self) -> Json {
        let kinds: serde_json::Map<String, Json> = self
            .kinds
            .iter()
            .map(|(k, b)| (k.clone(), json!(b.as_str())))
            .collect();
        json!({ "behavior": self.behavior.as_str(), "contexts": self.contexts, "kinds": kinds })
    }
}

impl Default for Policy {
    /// `freeze`, in `default`.
    fn default() -> Policy {
        Policy::new(Behavior::Freeze)
    }
}

fn unknown(s: &str) -> String {
    format!("outOfContext: {s}: freeze, stop, continue or end")
}

/// One element's contexts, its own behaviour, and what is applied to it now (none while it runs).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Element {
    /// Its own contexts (empty: the kit's).
    pub contexts: Vec<String>,
    /// Its own behaviour (none: its kind's, else the kit's).
    pub behavior: Option<Behavior>,
    /// What was applied when it left its contexts (none: it runs). `Continue` is kept too, so that coming back
    /// undoes nothing.
    pub applied: Option<Behavior>,
    /// The kit's time it left its contexts (what a freeze shifts by when it comes back).
    pub since: f64,
}

impl Element {
    /// The fields `contexts` and `outOfContext` of an element's own object (absent: the kit's).
    pub fn read(v: &Json) -> Result<Element, String> {
        let mut e = Element::default();
        e.set(v)?;
        Ok(e)
    }
    /// `contexts` and `outOfContext` of `v` over this element (`outOfContext: null` goes back to the kit's).
    pub fn set(&mut self, v: &Json) -> Result<(), String> {
        if let Some(c) = v.get("contexts") {
            self.contexts = contexts_of(c);
        }
        match v.get("outOfContext") {
            None => {}
            Some(Json::Null) => self.behavior = None,
            Some(b) => {
                self.behavior = Some(Behavior::of(b).ok_or_else(|| unknown(&b.to_string()))?)
            }
        }
        Ok(())
    }
    /// Whether something is applied (it does not run as usual: frozen, stopped).
    pub fn held(&self) -> bool {
        matches!(
            self.applied,
            Some(Behavior::Freeze | Behavior::Stop | Behavior::End)
        )
    }
    /// The contexts it runs in, `policy`'s when it names none.
    pub fn contexts_in<'a>(&'a self, policy: &'a Policy) -> &'a [String] {
        if self.contexts.is_empty() {
            &policy.contexts
        } else {
            &self.contexts
        }
    }
}

/// What a change of the top asks of one element: undo what was applied (`back`), then apply a behaviour (`leave`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Turn {
    pub back: Option<Behavior>,
    pub leave: Option<Behavior>,
}

impl Turn {
    pub fn is_none(&self) -> bool {
        self.back.is_none() && self.leave.is_none()
    }
}

/// The turn of element `e` of kind `kind` when `top` is on top: it leaves when it does not hear `top` and nothing is
/// applied; it comes back when it hears `top` and something is; a behaviour changed while out is undone then applied
/// again. The behaviour of `leave` is the one before the hook.
pub fn turn(policy: &Policy, e: &Element, kind: &str, top: &str) -> Turn {
    let out = !hears(e.contexts_in(policy), top);
    let want = policy.behavior_for(e.behavior, kind);
    match (out, e.applied) {
        (true, None) => Turn {
            back: None,
            leave: Some(want),
        },
        (true, Some(b)) if b != want && e.behavior.is_some() => Turn {
            back: Some(b),
            leave: Some(want),
        },
        (false, Some(b)) => Turn {
            back: Some(b),
            leave: None,
        },
        _ => Turn::default(),
    }
}

/// The context on top, as game.engine last told it (`default` until it says one).
#[derive(Clone, Debug, Default)]
pub struct Top {
    name: String,
}

impl Top {
    pub fn get(&self) -> &str {
        if self.name.is_empty() {
            DEFAULT
        } else {
            &self.name
        }
    }
    /// The top of the payload of [`CONTEXT_EVENT`]; whether it changed.
    pub fn set_from(&mut self, payload: &Json) -> bool {
        let top = payload["top"].as_str().unwrap_or(DEFAULT);
        if self.get() == top {
            return false;
        }
        self.name = top.to_string();
        true
    }
}

impl Kx<'_> {
    /// Subscribes `on_event(CONTEXT_EVENT)` to game.engine's stack, heard in every context; nothing without
    /// game.engine. Whether it subscribed.
    pub fn follow_contexts(&mut self) -> bool {
        if !self.host.has_service("game.engine.contexts") {
            return false;
        }
        self.on_in(CONTEXT_EVENT, CONTEXT_EVENT, &[ALL]);
        true
    }

    /// The behaviour element `element` (of kind `kind`) does as it leaves its contexts, after the kit's hook `hook`
    /// (`{ element, kind, behavior, top, contexts }`: `behavior` changeable, cancel = `continue`), asked only when a
    /// tap listens.
    pub fn leave_context(
        &mut self,
        hook: &str,
        element: &str,
        kind: &str,
        behavior: Behavior,
        top: &str,
        contexts: &[String],
    ) -> Behavior {
        if !self.hook_tapped(hook) {
            return behavior;
        }
        let asked = self.decide(
            hook,
            json!({ "element": element, "kind": kind, "behavior": behavior.as_str(), "top": top, "contexts": contexts }),
        );
        if asked["cancelled"] == Json::Bool(true) {
            return Behavior::Continue;
        }
        Behavior::of(&asked["payload"]["behavior"]).unwrap_or(behavior)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_element_leaves_comes_back_and_a_changed_behaviour_is_applied_again() {
        let mut p = Policy::new(Behavior::Freeze);
        let mut e = Element::default();
        assert!(turn(&p, &e, "", "default").is_none());
        let t = turn(&p, &e, "", "menu");
        assert_eq!(t.leave, Some(Behavior::Freeze));
        e.applied = t.leave;
        assert!(
            turn(&p, &e, "", "pause").is_none(),
            "still out: nothing again"
        );
        e.behavior = Some(Behavior::Stop);
        let t = turn(&p, &e, "", "pause");
        assert_eq!(
            (t.back, t.leave),
            (Some(Behavior::Freeze), Some(Behavior::Stop))
        );
        e.applied = Some(Behavior::Stop);
        let t = turn(&p, &e, "", "default");
        assert_eq!((t.back, t.leave), (Some(Behavior::Stop), None));
        e.applied = None;
        e.contexts = vec!["default".into(), "dialogue".into()];
        assert!(
            turn(&p, &e, "", "dialogue").is_none(),
            "it runs in dialogue"
        );
        p.apply(&json!({ "kinds": { "ui": "continue" } })).unwrap();
        e.contexts.clear();
        e.behavior = None;
        assert_eq!(turn(&p, &e, "ui", "menu").leave, Some(Behavior::Continue));
        assert!(p.apply(&json!("sleep")).is_err());
    }

    #[test]
    fn contexts_hear_their_names_all_and_default() {
        assert!(hears(&[], "default") && !hears(&[], "menu"));
        assert!(hears(&["all".into()], "menu"));
        assert!(hears(&["default".into(), "dialogue".into()], "dialogue"));
        assert_eq!(contexts_of(&json!("menu")), vec!["menu".to_string()]);
        let e = Element::read(&json!({ "contexts": ["dialogue"], "outOfContext": "end" })).unwrap();
        assert_eq!((e.contexts.len(), e.behavior), (1, Some(Behavior::End)));
    }
}
