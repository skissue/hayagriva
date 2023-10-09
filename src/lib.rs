/*!
Hayagriva provides a YAML-backed format and data model for various
bibliography items as well as formatting for both in-text citations and
reference lists based on these literature databases.

The crate is intended to assist scholarly writing and reference management
and can be used both through a CLI and an API.

Below, there is an example of how to parse a YAML database and get a Modern
Language Association-style citation.

# Supported styles

- Institute of Electrical and Electronics Engineers (IEEE)
    - [References](style::Ieee)
    - [Numerical citations](style::Numerical)
- Modern Language Association (MLA), 8th edition of the MLA Handbook
    - ["Works Cited" references](style::Mla)
- Chicago Manual of Style (CMoS), 17th edition
    - [Notes and Bibliography](style::ChicagoNotes)
    - [Author-Date references and citations](style::ChicagoAuthorDate)
- American Psychological Association (APA), 7th edition of the APA Publication Manual
    - [References](style::Apa)
- Other in-text citation styles
    - [Alphanumerical](style::Alphanumerical) (e. g. "Rass97")
    - [Author Title](style::AuthorTitle)

# Usage

```rust
use hayagriva::io::from_yaml_str;
use hayagriva::style::{Database, Mla};

let yaml = r#"
crazy-rich:
    type: Book
    title: Crazy Rich Asians
    author: Kwan, Kevin
    date: 2014
    publisher: Anchor Books
    location: New York, NY, US
"#;

// Parse a bibliography
let bib = from_yaml_str(yaml).unwrap();
assert_eq!(bib.get("crazy-rich").unwrap().date().unwrap().year, 2014);

// Format the reference
let db = bib.database();
let mut mla = Mla::new();
let reference = db.bibliography(&mut mla, None);
assert_eq!(reference[0].display.value, "Kwan, Kevin. Crazy Rich Asians. Anchor Books, 2014.");
```

Formatting for in-text citations is available through implementors of the
[`style::CitationStyle`] trait whereas bibliographies can be created by
[`style::BibliographyStyle`]. Both traits are used through a
[`style::Database`] which provides methods to format its records as
bibliographies and citations using references to implementors to these
traits.

If the default features are enabled, Hayagriva supports BibTeX and BibLaTeX
bibliographies. You can use [`io::from_biblatex_str`] to parse such
bibliographies.

Should you need more manual control, the library's native `Entry` struct
also offers an implementation of the `From<&biblatex::Entry>`-Trait. You will
need to depend on the [biblatex](https://docs.rs/biblatex/latest/biblatex/)
crate to obtain its `Entry`. Therefore, you could also use your BibLaTeX
content like this:

```ignore
use hayagriva::Entry;
let converted: Entry = your_biblatex_entry.into();
```

If you do not need BibLaTeX compatibility, you can use Hayagriva without the
default features by writing this in your `Cargo.toml`:

```toml
[dependencies]
hayagriva = { version = "0.2", default-features = false }
```

# Selectors

Hayagriva uses a custom selector language that enables you to filter
bibliographies by type of media. For more information about selectors, refer
to the [selectors.md
file](https://github.com/typst/hayagriva/blob/main/docs/selectors.md). While
you can parse user-defined selectors using the function `Selector::parse`,
you may instead want to use the selector macro to avoid the run time cost of
parsing a selector when working with constant selectors.

```rust
use hayagriva::select;
use hayagriva::io::from_yaml_str;

let yaml = r#"
quantized-vortex:
    type: Article
    author: Gross, E. P.
    title: Structure of a Quantized Vortex in Boson Systems
    date: 1961-05
    page-range: 454-477
    doi: 10.1007/BF02731494
    parent:
        issue: 3
        volume: 20
        title: Il Nuovo Cimento
"#;

let entries = from_yaml_str(yaml).unwrap();
let journal = select!((Article["date"]) > ("journal":Periodical));
assert!(journal.matches(entries.nth(0).unwrap()));
```

There are two ways to check if a selector matches an entry.
You should use [`Selector::matches`] if you just want to know if an item
matches a selector and [`Selector::apply`] to continue to work with the data from
parents of a matching entry. Keep in mind that the latter function will
return `Some` even if no sub-entry was bound / if the hash map is empty.
*/

#![warn(missing_docs)]
#![allow(clippy::comparison_chain)]

#[macro_use]
mod selectors;
#[cfg(feature = "biblatex")]
mod interop;

mod csl;
pub mod io;
pub mod lang;
pub mod style;
pub mod types;
mod util;

use indexmap::IndexMap;
pub use selectors::{Selector, SelectorError};

use paste::paste;
use serde::{de::Visitor, Deserialize, Serialize};
use types::*;
use unic_langid::LanguageIdentifier;
use util::{
    deserialize_one_or_many_opt, serialize_one_or_many, serialize_one_or_many_opt,
    OneOrMany,
};

/// A collection of bibliographic entries.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Library(IndexMap<String, Entry>);

impl Library {
    /// Construct a new, empty bibliography library.
    pub fn new() -> Self {
        Self(IndexMap::new())
    }

    /// Add an entry to the library.
    pub fn push(&mut self, entry: &Entry) {
        self.0.insert(entry.key.clone(), entry.clone());
    }

    /// Retrieve an entry from the library.
    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.0.get(key)
    }

    /// Get an iterator over the entries in the library.
    pub fn iter(&self) -> impl Iterator<Item = &Entry> {
        self.0.values()
    }

    /// Get an iterator over the keys in the library.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(|k| k.as_str())
    }

    /// Remove an entry from the library.
    pub fn remove(&mut self, key: &str) -> Option<Entry> {
        self.0.remove(key)
    }

    /// Get the length of the library.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Check whether the library is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Get the bibliography as a [`style::Database`].
    pub fn database(&self) -> style::Database {
        style::Database::from_entries(self.iter())
    }

    /// Get the nth entry in the library.
    pub fn nth(&self, n: usize) -> Option<&Entry> {
        self.0.get_index(n).map(|(_, v)| v)
    }
}

impl IntoIterator for Library {
    type Item = Entry;
    type IntoIter = std::iter::Map<
        indexmap::map::IntoIter<String, Entry>,
        fn((String, Entry)) -> Entry,
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter().map(|(_, v)| v)
    }
}

macro_rules! entry {
    ($(
        $(#[doc = $doc:literal])*
        $(#[serde $serde:tt])*
        $s:literal => $i:ident : $t:ty
        $(| $d:ty)? $(,)?
    ),*) => {
        // Build the struct and make it serializable.

        /// A citable item in a bibliography.
        #[derive(Debug, Clone, PartialEq, Serialize)]
        pub struct Entry {
            /// The key of the entry.
            #[serde(skip)]
            key: String,
            /// The type of the item.
            #[serde(rename = "type")]
            entry_type: EntryType,
            $(
                $(#[doc = $doc])*
                $(#[serde $serde])*
                #[serde(skip_serializing_if = "Option::is_none")]
                #[serde(rename = $s)]
                $i: Option<$t>,
            )*
            /// Item in which the item was published / to which it is strongly
            /// associated to.
            #[serde(serialize_with = "serialize_one_or_many")]
            #[serde(skip_serializing_if = "Vec::is_empty")]
            #[serde(rename = "parent")]
            parents: Vec<Entry>,
        }

        impl Entry {
            /// Get the key of the entry.
            pub fn key(&self) -> &str {
                &self.key
            }

            /// Construct a new, empty entry.
            pub fn new(key: &str, entry_type: EntryType) -> Self {
                Self {
                    key: key.to_owned(),
                    entry_type,
                    $(
                        $i: None,
                    )*
                    parents: Vec::new(),
                }
            }

            /// Check whether the entry has some key.
            pub fn has(&self, key: &str) -> bool {
                match key {
                    $(
                        $s => self.$i.is_some(),
                    )*
                    _ => false,
                }
            }
        }

        /// Getters.
        impl Entry {
            /// Get the type of the entry.
            pub fn entry_type(&self) -> &EntryType {
                &self.entry_type
            }

            /// Get the parents of the entry.
            pub fn parents(&self) -> &[Entry] {
                &self.parents
            }

            $(
                entry!(@get $(#[doc = $doc])* $s => $i : $t $(| $d)?);
            )*
        }

        /// Setters.
        impl Entry {
            /// Set the parents of the entry.
            pub fn set_parents(&mut self, parents: Vec<Entry>) {
                self.parents = parents;
            }


            $(
                entry!(@set $s => $i : $t);
            )*
        }

        /// The library deserialization also handles entries.
        ///
        /// Entries do not implement [`Deserialize`] because they have a data
        /// dependency on their key (stored in the parent map) and their
        /// children for default types.
        impl<'de> Deserialize<'de> for Library {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct MyVisitor;

                #[derive(Deserialize)]
                struct NakedEntry {
                    #[serde(rename = "type")]
                    entry_type: Option<EntryType>,
                    #[serde(default)]
                    #[serde(rename = "parent")]
                    parents: OneOrMany<NakedEntry>,
                    $(
                        $(#[serde $serde])*
                        #[serde(rename = $s)]
                        #[serde(default)]
                        $i: Option<$t>,
                    )*
                }

                impl NakedEntry {
                    /// Convert into a full entry using the child entry type
                    /// (if any) and the key.
                    fn into_entry<E>(
                        self,
                        key: &str,
                        child_entry_type: Option<EntryType>,
                    ) -> Result<Entry, E>
                        where E: serde::de::Error
                    {
                        let entry_type = self.entry_type
                            .or_else(|| child_entry_type.map(|e| e.default_parent()))
                            .ok_or_else(|| E::custom("no entry type"))?;

                        let parents: Result<Vec<_>, _> = self.parents
                            .into_iter()
                            .map(|p| p.into_entry(key, Some(entry_type)))
                            .collect();

                        Ok(Entry {
                            key: key.to_owned(),
                            entry_type,
                            parents: parents?,
                            $(
                                $i: self.$i,
                            )*
                        })
                    }
                }

                impl<'de> Visitor<'de> for MyVisitor {
                    type Value = Library;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter)
                        -> std::fmt::Result
                    {
                        formatter.write_str(
                            "a map between cite keys and entries"
                        )
                    }

                    fn visit_map<A>(self, mut map: A)
                        -> Result<Self::Value, A::Error>
                    where
                        A: serde::de::MapAccess<'de>,
                    {
                        let mut entries = Vec::with_capacity(
                            map.size_hint().unwrap_or(0).min(128)
                        );
                        while let Some(key) = map.next_key::<String>()? {
                            if entries.iter().any(|(k, _)| k == &key) {
                                return Err(serde::de::Error::custom(format!(
                                    "duplicate key {}",
                                    key
                                )));
                            }

                            let entry: NakedEntry = map.next_value()?;
                            entries.push((key, entry));
                        }

                        let entries: Result<IndexMap<_, _>, A::Error> =
                            entries.into_iter().map(|(k, v)| {
                                v.into_entry(&k, None).map(|e| (k, e))
                            }).collect();

                        Ok(Library(entries?))
                    }
                }

                deserializer.deserialize_map(MyVisitor)
            }
        }
    };

    (@match
        $s:literal => $i:ident,
        $naked:ident, $map:ident $(,)?
    ) => {
        $naked.$i = Some($map.next_value()?)
    };

    // All items with a serde attribute are expected to be collections.
    (@match
        $(#[serde $serde:tt])+
        $s:literal => $i:ident,
        $naked:ident, $map:ident $(,)?
    ) => {
        let one_or_many: OneOrMany = $map.next_value()?;
        $naked.$i = Some(one_or_many.into());
    };

    // Getter macro for deref types
    (@get $(#[$docs:meta])+ $s:literal => $i:ident : $t:ty | $d:ty $(,)?) => {
            $(#[$docs])+
            pub fn $i(&self) -> Option<&$d> {
                self.$i.as_deref()
            }
    };

    // Getter macro for regular types.
    (@get $(#[$docs:meta])+ $s:literal => $i:ident : $t:ty $(,)?) => {
        $(#[$docs])+
        pub fn $i(&self) -> Option<&$t> {
            self.$i.as_ref()
        }
    };

    // Setter for all types.
    (@set $s:literal => $i:ident : $t:ty $(,)?) => {
        paste! {
            #[doc = "Set the `" $s "` field."]
            pub fn [<set_ $i>](&mut self, $i: $t) {
                self.$i = Some($i);
            }
        }
    };
}

entry! {
    /// Title of the item.
    "title" => title: FormatString,
    /// Persons primarily responsible for creating the item.
    #[serde(serialize_with = "serialize_one_or_many_opt")]
    #[serde(deserialize_with = "deserialize_one_or_many_opt")]
    "author" => authors: Vec<Person> | [Person],
    /// Date at which the item was published.
    "date" => date: Date,
    /// Persons responsible for selecting and revising the content of the item.
    #[serde(serialize_with = "serialize_one_or_many_opt")]
    #[serde(deserialize_with = "deserialize_one_or_many_opt")]
    "editor" => editors: Vec<Person> | [Person],
    /// Persons involved in the production of the item that are not authors or editors.
    #[serde(serialize_with = "serialize_one_or_many_opt")]
    #[serde(deserialize_with = "deserialize_one_or_many_opt")]
    "affiliated" => affiliated: Vec<PersonsWithRoles> | [PersonsWithRoles],
    /// Publisher of the item.
    "publisher" => publisher: FormatString,
    /// Physical location at which the item was published or created.
    "location" => location: FormatString,
    /// Organization at/for which the item was created.
    "organization" => organization: FormatString,
    /// For an item whose parent has multiple issues, indicates the position in
    /// the issue sequence. Also used to indicate the episode number for TV.
    "issue" => issue: MaybeTyped<Numeric>,
    /// For an item whose parent has multiple volumes/parts/seasons ... of which
    /// this item is one.
    "volume" => volume: MaybeTyped<Numeric>,
    /// Total number of volumes/parts/seasons ... this item consists of.
    "volume-total" => volume_total: Numeric,
    /// Published version of an item.
    "edition" => edition: MaybeTyped<Numeric>,
    /// The range of pages within the parent this item occupies
    "page-range" => page_range: Numeric,
    /// The total number of pages the item has.
    "page-total" => page_total: Numeric,
    /// The time range within the parent this item starts and ends at.
    "time-range" => time_range: MaybeTyped<DurationRange>,
    /// The total runtime of the item.
    "runtime" => runtime: MaybeTyped<Duration>,
    /// Canonical public URL of the item, can have access date.
    "url" => url: QualifiedUrl,
    /// The Digital Object Identifier of the item.
    "doi" => doi: String,
    /// Any serial number or version describing the item that is not appropriate
    /// for the fields doi, edition, isbn or issn (may be assigned by the author
    /// of the item; especially useful for preprint archives).
    "serial-number" => serial_number: String,
    /// International Standard Book Number (ISBN), prefer ISBN-13.
    "isbn" => isbn: String,
    /// International Standard Serial Number (ISSN).
    "issn" => issn: String,
    /// The language of the item.
    "language" => language: LanguageIdentifier,
    /// Name of the institution/collection where the item is kept.
    "archive" => archive: FormatString,
    /// Physical location of the institution/collection where the item is kept.
    "archive-location" => archive_location: FormatString,
    /// The call number of the item in the institution/collection.
    "call-number" => call_number: FormatString,
    /// Additional description to be appended in the bibliographic entry.
    "note" => note: FormatString,
}

impl Entry {
    /// Get and parse the `affiliated` field and only return persons of a given
    /// [role](PersonRole).
    pub(crate) fn affiliated_with_role(&self, role: PersonRole) -> Vec<Person> {
        self.affiliated
            .iter()
            .flatten()
            .cloned()
            .filter_map(
                |PersonsWithRoles { names, role: r }| {
                    if r == role {
                        Some(names)
                    } else {
                        None
                    }
                },
            )
            .flatten()
            .collect()
    }

    /// Get the unconverted value of a certain field from this entry or any of
    /// its parents.
    pub fn map<'a, F, T>(&'a self, mut f: F) -> Option<T>
    where
        F: FnMut(&'a Self) -> Option<T>,
    {
        if let Some(value) = f(self) {
            Some(value)
        } else {
            self.map_parents(f)
        }
    }

    /// Get the unconverted value of a certain field from the parents only by BFS.
    pub fn map_parents<'a, F, T>(&'a self, mut f: F) -> Option<T>
    where
        F: FnMut(&'a Self) -> Option<T>,
    {
        let mut path: Vec<usize> = vec![0];
        let up = |path: &mut Vec<usize>| {
            path.pop();
            if let Some(last) = path.last_mut() {
                *last += 1;
            }
        };

        'outer: loop {
            // Index parents with the items in path. If, at any level, the index
            // exceeds the number of parents, increment the index at the
            // previous level. If no other level remains, return.
            let Some(first_path) = path.first() else {
                return None;
            };

            if self.parents.len() <= *first_path {
                return None;
            }

            let mut item = &self.parents[*first_path];

            for i in 1..path.len() {
                if path[i] >= item.parents.len() {
                    up(&mut path);
                    continue 'outer;
                }
                item = &item.parents[path[i]];
            }

            if let Some(first_path) = path.first_mut() {
                *first_path += 1;
            }

            if let Some(value) = f(item) {
                return Some(value);
            }
        }
    }

    /// Will recursively get a date off either the entry or any of its ancestors.
    pub fn date_any(&self) -> Option<&Date> {
        self.map(|e| e.date.as_ref())
    }

    /// Will recursively get an URL off either the entry or any of its ancestors.
    pub fn url_any(&self) -> Option<&QualifiedUrl> {
        self.map(|e| e.url.as_ref())
    }

    /// Extract the social media handle for the nth author from their alias.
    /// Will make sure the handle starts with `@`.
    ///
    /// If the `user_index` is 0, the function will try to extract
    /// the handle from the URL.
    pub(crate) fn social_handle(&self, user_index: usize) -> Option<String> {
        if self.entry_type != EntryType::Post {
            return None;
        }

        let authors = self.authors.as_deref().unwrap_or_default();

        if user_index > 0 && user_index >= authors.len() {
            return None;
        }

        if let Some(alias) = &authors[user_index].alias {
            return if alias.starts_with('@') {
                Some(alias.clone())
            } else {
                Some(format!("@{}", alias))
            };
        }

        if user_index == 0 {
            if let Some(url) = self.url.as_ref().map(|u| &u.value) {
                if !matches!(url.host(), Some(url::Host::Domain("twitter.com" | "x.com")))
                {
                    return None;
                }

                if let Some(handle) = url.path_segments().and_then(|mut c| c.next()) {
                    return Some(format!("@{}", handle));
                }
            }
        }

        None
    }
}

#[cfg(feature = "biblatex")]
impl Entry {
    /// Adds a parent to the current entry. The parent
    /// list will be created if there is none.
    pub(crate) fn add_parent(&mut self, entry: Entry) {
        self.parents.push(entry);
    }

    /// Adds affiliated persons. The list will be created if there is none.
    pub(crate) fn add_affiliated_persons(
        &mut self,
        new_persons: (Vec<Person>, PersonRole),
    ) {
        let obj = PersonsWithRoles { names: new_persons.0, role: new_persons.1 };
        if let Some(affiliated) = &mut self.affiliated {
            affiliated.push(obj);
        } else {
            self.affiliated = Some(vec![obj]);
        }
    }

    pub(crate) fn parents_mut(&mut self) -> &mut [Entry] {
        &mut self.parents
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use style::Citation;

    use super::*;
    use crate::io::from_yaml_str;
    use crate::style::{Apa, ChicagoNotes, Ieee, Mla};

    #[test]
    fn apa() {
        let contents = fs::read_to_string("tests/basic.yml").unwrap();
        let entries = from_yaml_str(&contents).unwrap();
        let apa = Apa::new();

        let db = entries.database();
        for reference in db.bibliography(&apa, None) {
            println!("{:#}", reference.display);
        }
    }

    #[test]
    fn ieee() {
        let contents = fs::read_to_string("tests/basic.yml").unwrap();
        let entries = from_yaml_str(&contents).unwrap();
        let ieee = Ieee::new();

        let db = entries.database();
        for reference in db.bibliography(&ieee, None) {
            println!("{:#}", reference.display);
        }
    }

    #[test]
    fn mla() {
        let contents = fs::read_to_string("tests/basic.yml").unwrap();
        let entries = from_yaml_str(&contents).unwrap();
        let mla = Mla::new();

        let db = entries.database();
        for reference in db.bibliography(&mla, None) {
            println!("{:#}", reference.display);
        }
    }

    #[test]
    fn chicago_n() {
        let contents = fs::read_to_string("tests/basic.yml").unwrap();
        let entries = from_yaml_str(&contents).unwrap();
        let mut chicago = ChicagoNotes::default();

        let mut db = entries.database();
        for entry in entries.iter() {
            let citation = Citation::new(entry, None);
            println!("{:#}", db.citation(&mut chicago, &[citation]).display);
        }
    }

    #[test]
    fn chicago_b() {
        let contents = fs::read_to_string("tests/basic.yml").unwrap();
        let entries = from_yaml_str(&contents).unwrap();
        let chicago = ChicagoNotes::default();

        let db = entries.database();
        for reference in db.bibliography(&chicago, None) {
            println!("{:#}", reference.display);
        }
    }

    macro_rules! select_all {
        ($select:expr, $entries:tt, [$($key:expr),* $(,)*] $(,)*) => {
            let keys = [$($key,)*];
            let selector = Selector::parse($select).unwrap();
            for entry in $entries.iter() {
                let res = selector.apply(entry);
                if keys.contains(&entry.key.as_str()) {
                    if res.is_none() {
                        panic!("Key {} not found in results", entry.key);
                    }
                } else {
                    if res.is_some() {
                        panic!("Key {} found in results", entry.key);
                    }
                }
            }
        }
    }

    macro_rules! select {
        ($select:expr, $entries:tt >> $entry_key:expr, [$($key:expr),* $(,)*] $(,)*) => {
            let keys = vec![ $( $key , )* ];
            let entry = $entries.iter().filter_map(|i| if i.key == $entry_key {Some(i)} else {None}).next().unwrap();
            let selector = Selector::parse($select).unwrap();
            let res = selector.apply(entry).unwrap();
            if !keys.into_iter().all(|k| res.get(k).is_some()) {
                panic!("Results do not contain binding");
            }
        }
    }

    #[test]
    fn selectors() {
        let contents = fs::read_to_string("tests/basic.yml").unwrap();
        let entries = from_yaml_str(&contents).unwrap();

        select_all!("article > proceedings", entries, ["zygos"]);
        select_all!(
            "article > (periodical | newspaper)",
            entries,
            ["omarova-libra", "kinetics", "house", "swedish",]
        );
        select_all!(
            "(chapter | anthos) > (anthology | book)",
            entries,
            ["harry", "gedanken"]
        );
        select_all!(
            "*[url]",
            entries,
            [
                "omarova-libra",
                "science-e-issue",
                "oiseau",
                "georgia",
                "really-habitable",
                "electronic-music",
                "mattermost",
                "worth",
                "wrong",
                "un-hdr",
                "audio-descriptions",
                "camb",
                "logician",
                "dns-encryption",
                "overleaf",
                "editors",
            ]
        );
        select_all!(
            "!(*[url] | (* > *[url]))",
            entries,
            [
                "zygos",
                "harry",
                "terminator-2",
                "interior",
                "wire",
                "kinetics",
                "house",
                "plaque",
                "renaissance",
                "gedanken",
                "donne",
                "roe-wade",
                "foia",
                "drill",
                "swedish",
                "latex-users",
                "barb",
            ]
        );
    }

    #[test]
    fn selector_bindings() {
        let contents = fs::read_to_string("tests/basic.yml").unwrap();
        let entries = from_yaml_str(&contents).unwrap();

        select!(
            "a:article > (b:conference & c:(video|blog|web))",
            entries >> "wwdc-network",
            ["a", "b", "c"]
        );
    }
}
