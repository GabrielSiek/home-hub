//! Matching what someone said or typed against names (devices, rooms, apps, teams), in Portuguese
//! or English: accents folded, case and punctuation ignored.

/// Lower case without accents.
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'ª' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'º' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            c => c,
        })
        .collect()
}

/// `fold`, only letters and digits, one space between words.
pub fn words(s: &str) -> String {
    fold(s)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `words`, keeping "+" (as in "Disney+").
pub fn normalize(s: &str) -> String {
    let mapped: String = fold(s)
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '+' { c } else { ' ' })
        .collect();
    mapped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How well `name` answers to `query`: 0 (not at all) to 100 (same name).
pub fn score(query: &str, name: &str) -> u8 {
    let (q, n) = (normalize(query), normalize(name));
    if q.is_empty() || n.is_empty() {
        return 0;
    }
    if q == n {
        return 100;
    }
    if q.replace(' ', "") == n.replace(' ', "") {
        return 95;
    }
    if n.starts_with(&q) {
        return 80;
    }
    if n.contains(&q) {
        return 65;
    }
    if q.len() >= 3 && q.contains(&n) {
        return 55;
    }
    let words: Vec<&str> = n.split(' ').collect();
    if q.split(' ').all(|w| words.contains(&w)) {
        return 45;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_and_punctuation_fold_away() {
        assert_eq!(fold("Grêmio São Paulo"), "gremio sao paulo");
        assert_eq!(words("  Abajur — Sala (2)! "), "abajur sala 2");
        assert_eq!(normalize("Disney+ Ação"), "disney+ acao");
    }

    #[test]
    fn closer_names_score_higher() {
        assert_eq!(score("abajur sala", "Abajur Sala"), 100);
        assert_eq!(score("abajursala", "Abajur Sala"), 95);
        assert_eq!(score("abajur", "Abajur Sala"), 80);
        assert_eq!(score("sala", "Abajur Sala"), 65);
        assert_eq!(score("luz do quarto", "quarto"), 55);
        assert_eq!(score("sala abajur", "Abajur da Sala"), 45);
        assert_eq!(score("cozinha", "Abajur Sala"), 0);
        assert_eq!(score("", "Abajur"), 0);
    }
}
