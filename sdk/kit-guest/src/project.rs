//! A mounted project's immutable snapshot. Keep the contract's compact block and
//! decode only the fields/collections a kit actually reads. A complete JSON tree
//! per kit made large levels consume hundreds of megabytes before the first tick.
use gripforge_abi::{
    fixed::{Kept, Owned, View},
    Reader,
};
use serde_json::Value as Json;
use std::cell::OnceCell;
use std::ops::Deref;

enum Source {
    Common(Owned),
    Crossing(Kept),
    Json(Json),
}

pub struct Project {
    source: Source,
    fields: Vec<(String, OnceCell<Json>)>,
    collections: Vec<(String, OnceCell<Json>)>,
    full: OnceCell<Json>,
}

impl Project {
    pub(crate) fn read(r: &mut Reader) -> Result<Self, String> {
        let source = if let Some(block) = r.fixed()? {
            Source::Common(block)
        } else if let Some(block) = r.kept()? {
            Source::Crossing(block)
        } else {
            Source::Json(r.value()?)
        };
        let mut p = Self {
            source,
            fields: Vec::new(),
            collections: Vec::new(),
            full: OnceCell::new(),
        };
        if let Some(v) = p.view() {
            p.fields = v
                .entries()
                .map(|(key, _)| (key.to_owned(), OnceCell::new()))
                .collect();
            p.collections = p
                .view()
                .and_then(|v| v.get("data"))
                .map(|v| {
                    v.entries()
                        .map(|(key, _)| (key.to_owned(), OnceCell::new()))
                        .collect()
                })
                .unwrap_or_default();
        }
        Ok(p)
    }

    fn view(&self) -> Option<View<'_>> {
        match &self.source {
            Source::Common(b) => Some(b.view()),
            Source::Crossing(b) => Some(b.view()),
            Source::Json(_) => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        if let Source::Json(v) = &self.source {
            return v.get(key);
        }
        if let Some(v) = self.full.get() {
            return v.get(key);
        }
        let cell = &self.fields.iter().find(|(k, _)| k == key)?.1;
        Some(cell.get_or_init(|| {
            self.view()
                .and_then(|v| v.get(key))
                .and_then(|v| v.to_json().ok())
                .unwrap_or(Json::Null)
        }))
    }

    pub(crate) fn collection(&self, name: &str) -> Option<&Json> {
        if let Source::Json(v) = &self.source {
            return v.get("data")?.get(name);
        }
        if let Some(v) = self.full.get() {
            return v.get("data")?.get(name);
        }
        let cell = &self.collections.iter().find(|(k, _)| k == name)?.1;
        Some(cell.get_or_init(|| {
            self.view()
                .and_then(|v| v.get("data"))
                .and_then(|v| v.get(name))
                .and_then(|v| v.to_json().ok())
                .unwrap_or(Json::Null)
        }))
    }
}

impl Deref for Project {
    type Target = Json;
    fn deref(&self) -> &Json {
        if let Source::Json(v) = &self.source {
            return v;
        }
        self.full.get_or_init(|| {
            self.view()
                .and_then(|v| v.to_json().ok())
                .unwrap_or(Json::Null)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gripforge_abi::Writer;
    use serde_json::json;

    #[test]
    fn compact_projects_preserve_fields_collections_and_full_reads() {
        let value = json!({"id":"game", "name":"A", "data": {"missingId":[{"value":1}], "empty":[],
            "large":(0..10000).map(|i| json!({"id":i,"point":[i,2.5,-0.0]})).collect::<Vec<_>>(), "null":null}});
        for mode in 0..3 {
            let mut writer = Writer::to_peer(mode == 1, mode == 2);
            writer.value(&value);
            let p = Project::read(&mut Reader::from(writer.bytes(), mode == 1)).unwrap();
            assert_eq!(p.get("name"), value.get("name"));
            for key in ["empty", "missingId", "null", "absent"] {
                assert_eq!(p.collection(key), value["data"].get(key));
            }
            assert!(p.full.get().is_none());
            assert!(p
                .collections
                .iter()
                .find(|(k, _)| k == "large")
                .is_none_or(|(_, v)| v.get().is_none()));
            assert_eq!(p.collection("large"), value["data"].get("large"));
            assert_eq!(
                serde_json::to_string(&*p).unwrap(),
                serde_json::to_string(&value).unwrap()
            );
        }
    }

    #[test]
    fn replacing_a_snapshot_drops_cached_fields_and_reads_new_values() {
        let read = |v| {
            let mut w = Writer::passing(true);
            w.value(&v);
            Project::read(&mut Reader::from(w.bytes(), true)).unwrap()
        };
        let mut p = read(json!({"id":"old", "data":{"scenes":[1]}}));
        assert_eq!(p.collection("scenes"), Some(&json!([1])));
        p = read(json!({"id":"new", "data":{"scenes":[2]}}));
        assert_eq!(p.get("id"), Some(&json!("new")));
        assert_eq!(p.collection("scenes"), Some(&json!([2])));
    }
}
