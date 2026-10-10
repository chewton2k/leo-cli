use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use base64::Engine;
use leo_core::notes::Note;
use leo_core::store::Store;
use serde::{Deserialize, Serialize};

pub type Meaning = Arc<dyn Fn(&[String], bool) -> Result<Vec<Vec<f32>>> + Send + Sync>;

pub const PIECE_CHARS: usize = 1200;
pub const MOST_PIECES: usize = 12;
pub const PIECES_PER_TICK: usize = 256;
pub const CLOSE_ENOUGH: f32 = 0.62;
pub const NEIGHBOURS: usize = 24;
const MOST_FOR_NEIGHBOURS: usize = 6000;

pub type Near = BTreeMap<String, std::collections::BTreeSet<String>>;

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub hash: String,
    pub pieces: Vec<(usize, Vec<f32>)>,
}

#[derive(Serialize, Deserialize, Default)]
struct OnDisk {
    #[serde(default)]
    notes: BTreeMap<String, Stored>,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    hash: String,
    pieces: Vec<(usize, String)>,
}

pub struct Vectors {
    db: Arc<crate::db::Db>,
    known: Mutex<BTreeMap<String, Entry>>,
    version: std::sync::atomic::AtomicU64,
}

fn to_blob(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn from_blob(bytes: &[u8]) -> Option<Vec<f32>> {
    bytes.len().is_multiple_of(4).then(|| {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect()
    })
}

fn decode(text: &str) -> Option<Vec<f32>> {
    from_blob(
        &base64::engine::general_purpose::STANDARD
            .decode(text)
            .ok()?,
    )
}

pub fn hash_of(note: &Note) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in note.title.bytes().chain([0]).chain(note.body.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub fn pieces_of(note: &Note) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut current = String::new();
    let mut start = 0usize;
    let mut at = 0usize;
    for paragraph in note.body.split("\n\n") {
        let size = paragraph.chars().count();
        if !current.is_empty() && current.chars().count() + size > PIECE_CHARS {
            out.push((start, std::mem::take(&mut current)));
            start = at;
        }
        if size > PIECE_CHARS {
            let chars: Vec<char> = paragraph.chars().collect();
            for (n, part) in chars.chunks(PIECE_CHARS).enumerate() {
                out.push((at + n * PIECE_CHARS, part.iter().collect()));
            }
            start = at + size + 2;
        } else {
            if !current.is_empty() {
                current.push_str("\n\n");
            }
            current.push_str(paragraph);
        }
        at += size + 2;
    }
    if !current.trim().is_empty() || out.is_empty() {
        out.push((start, current));
    }
    if out.len() > 1 {
        out.retain(|(_, text)| !text.trim().is_empty());
    }
    out.truncate(MOST_PIECES);
    if let Some(first) = out.first_mut() {
        first.1 = format!("{}\n\n{}", note.title, first.1).trim().to_string();
    }
    out
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

pub struct Work {
    pub id: String,
    pub hash: String,
    pub pieces: Vec<(usize, String)>,
}

impl Vectors {
    pub fn for_notes(notes_dir: &Path) -> Vectors {
        let base = notes_dir.parent().unwrap_or(notes_dir);
        Vectors::at(base.join("meaning.json"))
    }

    pub fn at(path: PathBuf) -> Vectors {
        let db = crate::db::beside(&path);
        let old: Option<OnDisk> = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok());
        if let Some(old) = old {
            let saved = db.with(|c| {
                let tx = c.transaction()?;
                for (id, stored) in &old.notes {
                    for (at, v) in &stored.pieces {
                        if let Some(v) = decode(v) {
                            tx.execute(
                                "INSERT OR REPLACE INTO vectors (note, at, hash, v) VALUES (?1, ?2, ?3, ?4)",
                                rusqlite::params![id, *at as i64, stored.hash, to_blob(&v)],
                            )?;
                        }
                    }
                }
                tx.commit()
            });
            if saved.is_ok() {
                crate::db::put_aside(&path);
            }
        }
        let rows: Vec<(String, i64, String, Vec<u8>)> = db
            .with(|c| {
                let mut found =
                    c.prepare("SELECT note, at, hash, v FROM vectors ORDER BY note, at")?;
                let rows =
                    found.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
                rows.collect()
            })
            .unwrap_or_default();
        let mut known: BTreeMap<String, Entry> = BTreeMap::new();
        for (note, at, hash, v) in rows {
            let Some(v) = from_blob(&v) else { continue };
            known
                .entry(note)
                .or_insert_with(|| Entry {
                    hash: hash.clone(),
                    pieces: Vec::new(),
                })
                .pieces
                .push((at.max(0) as usize, v));
        }
        Vectors {
            db,
            known: Mutex::new(known),
            version: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub fn path(&self) -> &Path {
        self.db.path()
    }

    pub fn bytes(&self) -> u64 {
        self.db
            .with(|c| {
                c.query_row(
                    "SELECT coalesce(SUM(length(v) + length(note) + length(hash)), 0) FROM vectors",
                    [],
                    |r| r.get::<_, i64>(0),
                )
            })
            .map_or(0, |n| n.max(0) as u64)
    }

    pub fn clear(&self) -> bool {
        if let Ok(mut known) = self.known.lock() {
            known.clear();
        }
        self.changed();
        self.db
            .with(|c| c.execute("DELETE FROM vectors", []))
            .is_ok_and(|n| n > 0)
    }

    pub fn version(&self) -> u64 {
        self.version.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn changed(&self) {
        self.version
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn neighbours(&self, most: usize) -> Near {
        let Ok(known) = self.known.lock() else {
            return Near::new();
        };
        if known.len() > MOST_FOR_NEIGHBOURS {
            return Near::new();
        }
        let means: Vec<(&String, Vec<f32>)> = known
            .iter()
            .filter_map(|(id, entry)| {
                let width = entry.pieces.first()?.1.len();
                let mut sum = vec![0f32; width];
                for (_, v) in &entry.pieces {
                    sum.iter_mut().zip(v).for_each(|(s, x)| *s += x);
                }
                let norm = sum.iter().map(|x| x * x).sum::<f32>().sqrt();
                (norm > 0.0).then(|| (id, sum.into_iter().map(|x| x / norm).collect()))
            })
            .collect();
        let mut near = Near::new();
        for (id, v) in &means {
            let mut scored: Vec<(f32, &String)> = means
                .iter()
                .filter(|(other, _)| other != id)
                .map(|(other, w)| (cosine(v, w), *other))
                .collect();
            scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(b.1)));
            near.insert(
                (*id).clone(),
                scored
                    .into_iter()
                    .take(most)
                    .map(|(_, other)| other.clone())
                    .collect(),
            );
        }
        near
    }

    pub fn len(&self) -> usize {
        self.known.lock().map(|k| k.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn stale(&self, store: &Store, most_pieces: usize) -> Vec<Work> {
        let Ok(known) = self.known.lock() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut pieces = 0;
        for note in &store.notes {
            let hash = hash_of(note);
            if known.get(&note.id).is_some_and(|e| e.hash == hash) {
                continue;
            }
            let found = pieces_of(note);
            if pieces > 0 && pieces + found.len() > most_pieces {
                break;
            }
            pieces += found.len();
            out.push(Work {
                id: note.id.clone(),
                hash,
                pieces: found,
            });
        }
        out
    }

    pub fn forget_gone(&self, store: &Store) -> usize {
        let Ok(mut known) = self.known.lock() else {
            return 0;
        };
        let live: std::collections::HashSet<&str> =
            store.notes.iter().map(|n| n.id.as_str()).collect();
        let gone: Vec<String> = known
            .keys()
            .filter(|id| !live.contains(id.as_str()))
            .cloned()
            .collect();
        if gone.is_empty() {
            return 0;
        }
        for id in &gone {
            known.remove(id);
        }
        drop(known);
        let _ = self.db.with(|c| {
            let tx = c.transaction()?;
            for id in &gone {
                tx.execute("DELETE FROM vectors WHERE note = ?1", [id])?;
            }
            tx.commit()
        });
        self.changed();
        gone.len()
    }

    pub fn put(&self, work: &[Work], vectors: Vec<Vec<f32>>) {
        let Ok(mut known) = self.known.lock() else {
            return;
        };
        let mut given = vectors.into_iter();
        for item in work {
            let pieces: Vec<(usize, Vec<f32>)> = item
                .pieces
                .iter()
                .filter_map(|(at, _)| given.next().map(|v| (*at, v)))
                .collect();
            if pieces.len() != item.pieces.len() {
                continue;
            }
            let _ = self.db.with(|c| {
                let tx = c.transaction()?;
                tx.execute("DELETE FROM vectors WHERE note = ?1", [&item.id])?;
                for (at, v) in &pieces {
                    tx.execute(
                        "INSERT INTO vectors (note, at, hash, v) VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![item.id, *at as i64, item.hash, to_blob(v)],
                    )?;
                }
                tx.commit()
            });
            known.insert(
                item.id.clone(),
                Entry {
                    hash: item.hash.clone(),
                    pieces,
                },
            );
        }
        drop(known);
        self.changed();
    }

    pub fn nearest(&self, query: &[f32], most: usize, least: f32) -> Vec<(String, f32)> {
        let Ok(known) = self.known.lock() else {
            return Vec::new();
        };
        let mut scored: Vec<(String, f32)> = known
            .iter()
            .filter_map(|(id, entry)| {
                entry
                    .pieces
                    .iter()
                    .map(|(_, v)| cosine(query, v))
                    .fold(None, |best: Option<f32>, s| {
                        Some(best.map_or(s, |b| b.max(s)))
                    })
                    .filter(|s| *s >= least)
                    .map(|s| (id.clone(), s))
            })
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        scored.truncate(most);
        scored
    }
}

pub fn close_to(
    meaning: Option<&Meaning>,
    vectors: &Vectors,
    text: &str,
    most: usize,
) -> Vec<(String, f32)> {
    let Some(meaning) = meaning else {
        return Vec::new();
    };
    if vectors.is_empty() || text.trim().is_empty() {
        return Vec::new();
    }
    meaning(&[text.to_string()], true)
        .ok()
        .and_then(|mut v| v.pop())
        .map(|q| vectors.nearest(&q, most, CLOSE_ENOUGH))
        .unwrap_or_default()
}

pub fn catch_up(store: &Store, vectors: &Vectors, meaning: &Meaning, most_pieces: usize) -> usize {
    let work = vectors.stale(store, most_pieces);
    vectors.forget_gone(store);
    read_in(vectors, meaning, &work)
}

pub fn read_in(vectors: &Vectors, meaning: &Meaning, work: &[Work]) -> usize {
    if work.is_empty() {
        return 0;
    }
    let texts: Vec<String> = work
        .iter()
        .flat_map(|w| w.pieces.iter().map(|(_, t)| t.clone()))
        .collect();
    match meaning(&texts, false) {
        Ok(found) if found.len() == texts.len() => {
            vectors.put(work, found);
            work.len()
        }
        _ => 0,
    }
}

#[cfg(test)]
pub fn fake_meaning() -> Meaning {
    tests::fake()
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn fake() -> Meaning {
        Arc::new(|texts: &[String], _query: bool| {
            Ok(texts
                .iter()
                .map(|t| {
                    let t = t.to_lowercase();
                    let mut v = vec![
                        f32::from(u8::from(
                            t.contains("queue") || t.contains("bfs") || t.contains("breadth"),
                        )),
                        f32::from(u8::from(t.contains("plant") || t.contains("light"))),
                        0.1,
                    ];
                    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                    v.iter_mut().for_each(|x| *x /= norm);
                    v
                })
                .collect())
        })
    }

    fn note(title: &str, body: &str) -> Note {
        let mut store =
            Store::load_from(&tempfile::tempdir().unwrap().keep().join("notes")).unwrap();
        store.create_note(title, body, vec![], "").unwrap().clone()
    }

    #[test]
    fn every_note_knows_its_nearest_notes_and_the_list_is_rebuilt_only_after_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        let a = store
            .create_note("BFS", "queue", vec![], "")
            .unwrap()
            .id
            .clone();
        let b = store
            .create_note("Breadth first", "queue order", vec![], "")
            .unwrap()
            .id
            .clone();
        let c = store
            .create_note("Plants", "light", vec![], "")
            .unwrap()
            .id
            .clone();
        let vectors = Vectors::for_notes(&store.notes_dir);
        let before = vectors.version();
        catch_up(&store, &vectors, &fake(), 100);
        assert!(vectors.version() > before);
        let near = vectors.neighbours(1);
        assert_eq!(near[&a].iter().next(), Some(&b));
        assert_eq!(near[&c].len(), 1);
        let graphs = crate::graph::Graphs::new(dir.path().join("graph.json"), None)
            .with_vectors(Arc::new(Vectors::for_notes(&store.notes_dir)));
        let first = graphs.near().unwrap();
        assert!(
            Arc::ptr_eq(&first, &graphs.near().unwrap()),
            "kept until the vectors change"
        );
    }

    #[test]
    fn a_note_is_cut_into_pieces_at_paragraphs_with_the_title_first() {
        let body = format!(
            "{}\n\n{}\n\n{}",
            "a".repeat(700),
            "b".repeat(700),
            "c".repeat(3000)
        );
        let pieces = pieces_of(&note("Heaps", &body));
        assert!(pieces[0].1.starts_with("Heaps\n\naaa"));
        assert_eq!(pieces[1].0, 702, "pieces remember where they start");
        assert!(pieces
            .iter()
            .all(|(_, t)| t.chars().count() <= PIECE_CHARS + 10));
        assert_eq!(pieces.len(), 5);
        assert_eq!(pieces_of(&note("Empty", "")).len(), 1);
        let long: String = (0..40)
            .map(|i| format!("{} {i}\n\n", "x".repeat(900)))
            .collect();
        assert_eq!(pieces_of(&note("Long", &long)).len(), MOST_PIECES);
        let wide = "é".repeat(2500);
        assert_eq!(
            pieces_of(&note("Accents", &wide)).len(),
            3,
            "cut by characters, never bytes"
        );
    }

    #[test]
    fn notes_are_read_once_and_found_by_meaning_after_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        let bfs = store
            .create_note(
                "Graph search",
                "Breadth first takes the oldest from a queue.",
                vec![],
                "",
            )
            .unwrap()
            .id
            .clone();
        store
            .create_note("Biology", "Plants turn light into sugar.", vec![], "")
            .unwrap();
        let vectors = Vectors::for_notes(&store.notes_dir);
        let meaning = fake();
        assert_eq!(catch_up(&store, &vectors, &meaning, 100), 2);
        assert_eq!(
            catch_up(&store, &vectors, &meaning, 100),
            0,
            "nothing changed"
        );
        let again = Vectors::for_notes(&store.notes_dir);
        assert_eq!(again.len(), 2);
        let close = close_to(Some(&meaning), &again, "how does BFS work?", 5);
        assert_eq!(close.len(), 1);
        assert_eq!(close[0].0, bfs);
        store.find_note_mut(&bfs).unwrap().body = "Now about plants.".into();
        assert_eq!(
            again.stale(&store, 100).len(),
            1,
            "an edited note is read again"
        );
        let gone = store.notes[1].id.clone();
        store.delete_notes(&[gone]);
        assert_eq!(again.forget_gone(&store), 1);
        assert!(
            close_to(None, &again, "bfs", 5).is_empty(),
            "no model, no meaning search"
        );
    }
}
