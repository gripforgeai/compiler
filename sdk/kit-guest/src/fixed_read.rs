//! What the façade reads **in place** from the fixed-layout values of the common memory (docs/kit-modules.md
//! §9.7): the typed things a kit is handed — an entity, a zone — are made straight from the block the core passed,
//! with no `serde_json::Value` built in between. Only what the kit keeps is copied (the texts, and the values of
//! the components, which a kit reads as `&Value`). Each function answers exactly what its `*_of(&Value)` twin
//! answers on the same value written out (a kit in its own memory, a kit on a Worker).

use crate::entities::Entity;
use gripforge_abi::fixed::{tag, View};
use gripforge_abi::Reader;
use serde_json::{Map, Value as Json};

fn text(v: View, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string()
}

/// `entities::entity_of` on a block: `{ id, kind, tags, components, owner, refs }`.
pub(crate) fn entity_of(v: View) -> Option<Entity> {
    if v.tag() != tag::DICT {
        return None;
    }
    let tags = v
        .get("tags")
        .map(|t| {
            t.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let components = match v.get("components") {
        Some(c) if c.tag() == tag::DICT => {
            let mut out: Vec<(String, Json)> = Vec::with_capacity(c.len());
            for (k, x) in c.entries() {
                out.push((k.to_string(), x.to_json().ok()?));
            }
            out
        }
        _ => Vec::new(),
    };
    let refs = match v
        .get("refs")
        .filter(|r| r.tag() == tag::DICT)
        .map(|r| r.to_json())
    {
        Some(Ok(Json::Object(o))) => o,
        _ => Map::new(),
    };
    Some(Entity {
        id: text(v, "id"),
        kind: text(v, "kind"),
        tags,
        components,
        owner: text(v, "owner"),
        refs,
    })
}

/// The entity the next value of a message holds: read in place when it came as a block, else from the value
/// written out.
pub(crate) fn entity(r: &mut Reader) -> Option<Entity> {
    match r.fixed().ok()? {
        Some(block) => entity_of(block.view()),
        None => r.value().ok().and_then(crate::entities::entity_of),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gripforge_abi::Writer;
    use serde_json::json;

    #[test]
    fn an_entity_read_in_place_is_the_entity_read_from_the_value() {
        for v in [
            json!({ "id": "e1", "kind": "mob", "tags": ["a", 3, "b"], "components": { "z": { "hp": 3 }, "a": [1, 2.0], "s": "t" }, "owner": "p1", "refs": { "r": "e2", "n": 1 } }),
            json!({ "id": "e2", "components": {} }),
            json!({ "id": "e3", "tags": "no", "components": [1], "refs": [], "kind": 4 }),
            json!([1, 2]),
        ] {
            let mut w = Writer::passing(true);
            w.value(&v);
            let got = entity(&mut Reader::from(w.bytes(), true));
            let want = crate::entities::entity_of(v.clone());
            assert_eq!(got.is_some(), want.is_some(), "{v}");
            if let (Some(a), Some(b)) = (got, want) {
                assert_eq!(
                    (&a.id, &a.kind, &a.tags, &a.owner),
                    (&b.id, &b.kind, &b.tags, &b.owner),
                    "{v}"
                );
                assert_eq!(
                    serde_json::to_string(&a.components).unwrap(),
                    serde_json::to_string(&b.components).unwrap(),
                    "{v}"
                );
                assert_eq!(
                    serde_json::to_string(&a.refs).unwrap(),
                    serde_json::to_string(&b.refs).unwrap(),
                    "{v}"
                );
            }
            // Written out (a kit in its own memory): the same function, the same entity.
            let mut plain = Writer::new();
            plain.value(&v);
            assert_eq!(
                entity(&mut Reader::from(plain.bytes(), false)).map(|e| e.id),
                crate::entities::entity_of(v).map(|e| e.id)
            );
        }
    }
}
