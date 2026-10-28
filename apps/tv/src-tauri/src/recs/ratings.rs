use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;

const MAX_RATINGS: usize = 2000;

/// What the user thought of a movie or series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Score {
    Dislike,
    Like,
    Love,
}

/// A rated title. Ratings belong to the title, not to the service it was watched on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rating {
    /// "movie" or "tv"
    pub media_type: String,
    pub id: u64,
    pub title: String,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    pub score: Score,
    /// Unix time in milliseconds.
    #[serde(default)]
    pub at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Ratings {
    #[serde(default)]
    pub list: Vec<Rating>,
}

impl Ratings {
    pub fn load(dir: &Path) -> Self {
        fs::read_to_string(dir.join("ratings.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        fs::write(dir.join("ratings.json"), serde_json::to_string(self)?)?;
        Ok(())
    }

    pub fn get(&self, media_type: &str, id: u64) -> Option<Score> {
        self.list
            .iter()
            .find(|r| r.media_type == media_type && r.id == id)
            .map(|r| r.score)
    }

    /// Rates the title, newest first; `None` takes the rating away.
    pub fn set(&mut self, rating: Rating, score: Option<Score>) {
        self.list.retain(|r| !(r.media_type == rating.media_type && r.id == rating.id));
        if let Some(score) = score {
            self.list.insert(0, Rating { score, ..rating });
            self.list.truncate(MAX_RATINGS);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rating(id: u64) -> Rating {
        Rating {
            media_type: "movie".into(),
            id,
            title: "Dune".into(),
            poster: None,
            backdrop: None,
            score: Score::Like,
            at: 1,
        }
    }

    #[test]
    fn rating_again_replaces_and_none_clears() {
        let mut r = Ratings::default();
        r.set(rating(1), Some(Score::Like));
        r.set(rating(2), Some(Score::Dislike));
        r.set(rating(1), Some(Score::Love));
        assert_eq!(r.list.len(), 2);
        assert_eq!(r.list[0].id, 1);
        assert_eq!(r.get("movie", 1), Some(Score::Love));
        assert_eq!(r.get("tv", 1), None);
        r.set(rating(1), None);
        assert_eq!(r.get("movie", 1), None);
        assert_eq!(r.list.len(), 1);
    }

    #[test]
    fn scores_are_lowercase_json() {
        assert_eq!(serde_json::to_string(&Score::Love).unwrap(), "\"love\"");
        let s: Score = serde_json::from_str("\"dislike\"").unwrap();
        assert_eq!(s, Score::Dislike);
    }
}
