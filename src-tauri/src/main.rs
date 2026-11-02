#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    env,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::UNIX_EPOCH,
};
use walkdir::WalkDir;

static BLOCK_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\n\s*\n").unwrap());
static HTML_TAG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]+>").unwrap());
static SSA_TAG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\{\\[^}]+\}").unwrap());
static SPACE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());

static CACHE: Lazy<Mutex<HashMap<PathBuf, CachedSubtitle>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Clone)]
struct Cue {
    start_ms: u64,
    end_ms: u64,
    text: String,
}

#[derive(Clone)]
struct CachedSubtitle {
    modified_ms: u128,
    cues: Vec<Cue>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LibraryItem {
    name: String,
    file_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchResult {
    collection: String,
    file_name: String,
    relative_path: String,
    start_ms: u64,
    end_ms: u64,
    text: String,
    score: f64,
    match_kind: String,
}

fn app_base_dir() -> Result<PathBuf, String> {
    if let Ok(custom) = env::var("SUBTITLE_SEARCHER_HOME") {
        return Ok(PathBuf::from(custom));
    }

    if cfg!(debug_assertions) {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        if let Some(project_root) = manifest.parent() {
            return Ok(project_root.to_path_buf());
        }
    }

    let exe = env::current_exe().map_err(|e| format!("Could not locate executable: {e}"))?;

    #[cfg(target_os = "macos")]
    {
        for ancestor in exe.ancestors() {
            if ancestor.extension().and_then(|e| e.to_str()) == Some("app") {
                if let Some(parent) = ancestor.parent() {
                    return Ok(parent.to_path_buf());
                }
            }
        }
    }

    exe.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "Could not determine application folder".to_string())
}

fn shows_dir() -> Result<PathBuf, String> {
    let dir = app_base_dir()?.join("Shows");
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Could not create Shows folder at {}: {e}", dir.display()))?;
    Ok(dir)
}

fn is_subtitle(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase()),
        Some(ext) if ext == "srt" || ext == "vtt"
    )
}

fn subtitle_files(folder: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = WalkDir::new(folder)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file() && is_subtitle(entry.path()))
        .map(|entry| entry.into_path())
        .collect();

    files.sort_by(|a, b| {
        a.to_string_lossy()
            .to_ascii_lowercase()
            .cmp(&b.to_string_lossy().to_ascii_lowercase())
    });
    files
}

#[tauri::command]
fn library_root() -> Result<String, String> {
    Ok(shows_dir()?.to_string_lossy().into_owned())
}

#[tauri::command]
fn scan_library() -> Result<Vec<LibraryItem>, String> {
    let root = shows_dir()?;
    let mut items = Vec::new();

    for entry in fs::read_dir(&root)
        .map_err(|e| format!("Could not read Shows folder {}: {e}", root.display()))?
    {
        let entry = entry.map_err(|e| format!("Could not read library item: {e}"))?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().into_owned();
        let file_count = subtitle_files(&path).len();
        items.push(LibraryItem { name, file_count });
    }

    items.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    Ok(items)
}

fn decode_entities(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn clean_cue_text(text: &str) -> String {
    let without_html = HTML_TAG_RE.replace_all(text, "");
    let without_ssa = SSA_TAG_RE.replace_all(&without_html, "");
    let decoded = decode_entities(&without_ssa);
    SPACE_RE.replace_all(decoded.trim(), " ").into_owned()
}

fn parse_timestamp(value: &str) -> Option<u64> {
    let value = value.trim().split_whitespace().next()?;
    let value = value.replace(',', ".");
    let mut parts = value.split(':');

    let first = parts.next()?;
    let second = parts.next()?;
    let third = parts.next();

    let (hours, minutes, seconds_part) = match third {
        Some(seconds) => (
            first.parse::<u64>().ok()?,
            second.parse::<u64>().ok()?,
            seconds,
        ),
        None => (0, first.parse::<u64>().ok()?, second),
    };

    let mut second_parts = seconds_part.split('.');
    let seconds = second_parts.next()?.parse::<u64>().ok()?;
    let fraction = second_parts.next().unwrap_or("0");
    let millis = match fraction.len() {
        0 => 0,
        1 => fraction.parse::<u64>().ok()? * 100,
        2 => fraction.parse::<u64>().ok()? * 10,
        _ => fraction[..3].parse::<u64>().ok()?,
    };

    Some(((hours * 3600 + minutes * 60 + seconds) * 1000) + millis)
}

fn parse_subtitle_text(content: &str) -> Vec<Cue> {
    let normalized = content
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");

    let mut cues = Vec::new();

    for block in BLOCK_RE.split(&normalized) {
        let lines: Vec<&str> = block.lines().collect();
        let Some(time_index) = lines.iter().position(|line| line.contains("-->")) else {
            continue;
        };

        let Some((start_raw, end_raw)) = lines[time_index].split_once("-->") else {
            continue;
        };

        let Some(start_ms) = parse_timestamp(start_raw) else {
            continue;
        };
        let Some(end_ms) = parse_timestamp(end_raw) else {
            continue;
        };

        let text = lines
            .iter()
            .skip(time_index + 1)
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ");

        let text = clean_cue_text(&text);
        if !text.is_empty() {
            cues.push(Cue {
                start_ms,
                end_ms,
                text,
            });
        }
    }

    cues
}

fn file_modified_ms(path: &Path) -> u128 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn load_cues(path: &Path) -> Vec<Cue> {
    let modified_ms = file_modified_ms(path);

    if let Ok(cache) = CACHE.lock() {
        if let Some(cached) = cache.get(path) {
            if cached.modified_ms == modified_ms {
                return cached.cues.clone();
            }
        }
    }

    let content = match fs::read(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(_) => return Vec::new(),
    };
    let cues = parse_subtitle_text(&content);

    if let Ok(mut cache) = CACHE.lock() {
        cache.insert(
            path.to_path_buf(),
            CachedSubtitle {
                modified_ms,
                cues: cues.clone(),
            },
        );
    }

    cues
}

fn normalize_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for ch in text.to_lowercase().chars() {
        let ch = match ch {
            '’' | '‘' | '`' => '\'',
            _ => ch,
        };

        if ch.is_alphanumeric() || ch == '\'' {
            out.push(ch);
        } else {
            out.push(' ');
        }
    }

    SPACE_RE.replace_all(out.trim(), " ").into_owned()
}

fn tokens(text: &str) -> Vec<String> {
    normalize_text(text)
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect()
}

fn irregular_root(token: &str) -> Option<&'static str> {
    match token {
        "am" | "is" | "are" | "was" | "were" | "been" => Some("be"),
        "has" | "had" => Some("have"),
        "does" | "did" | "done" => Some("do"),
        "went" | "gone" => Some("go"),
        "came" => Some("come"),
        "took" | "taken" => Some("take"),
        "brought" => Some("bring"),
        "bought" => Some("buy"),
        "thought" => Some("think"),
        "found" => Some("find"),
        "made" => Some("make"),
        "said" => Some("say"),
        "told" => Some("tell"),
        "gave" | "given" => Some("give"),
        "got" | "gotten" => Some("get"),
        "putting" => Some("put"),
        "ran" => Some("run"),
        "saw" | "seen" => Some("see"),
        "left" => Some("leave"),
        "kept" => Some("keep"),
        "threw" | "thrown" => Some("throw"),
        "wrote" | "written" => Some("write"),
        "ate" | "eaten" => Some("eat"),
        "drank" | "drunk" => Some("drink"),
        "children" => Some("child"),
        "people" => Some("person"),
        _ => None,
    }
}

fn roots(token: &str) -> Vec<String> {
    let mut values = vec![token.to_string()];

    if let Some(root) = irregular_root(token) {
        values.push(root.to_string());
    }

    if token.ends_with("ies") && token.len() > 4 {
        values.push(format!("{}y", &token[..token.len() - 3]));
    }

    if token.ends_with("ing") && token.len() > 5 {
        let stem = &token[..token.len() - 3];
        values.push(stem.to_string());
        values.push(format!("{stem}e"));

        let chars: Vec<char> = stem.chars().collect();
        if chars.len() >= 2 && chars[chars.len() - 1] == chars[chars.len() - 2] {
            values.push(chars[..chars.len() - 1].iter().collect());
        }
    }

    if token.ends_with("ed") && token.len() > 4 {
        let stem = &token[..token.len() - 2];
        values.push(stem.to_string());
        values.push(token[..token.len() - 1].to_string());
        if token.ends_with("ied") {
            values.push(format!("{}y", &token[..token.len() - 3]));
        }
    }

    if token.ends_with('s') && !token.ends_with("ss") && token.len() > 3 {
        values.push(token[..token.len() - 1].to_string());
    }

    values.sort();
    values.dedup();
    values
}

fn synonym_root(token: &str) -> &str {
    match token {
        "garbage" | "rubbish" | "trash" => "trash",
        "sofa" | "couch" => "couch",
        "refrigerator" | "fridge" => "fridge",
        "restroom" | "washroom" | "bathroom" => "bathroom",
        "television" | "tv" => "tv",
        "mom" | "mum" | "mother" => "mother",
        "dad" | "father" => "father",
        "kid" | "child" => "child",
        "purchase" | "buy" => "buy",
        "cellphone" | "phone" | "telephone" => "phone",
        "movie" | "film" => "movie",
        _ => token,
    }
}

fn stopword(token: &str) -> bool {
    matches!(
        token,
        "a" | "an" | "the" | "i" | "you" | "he" | "she" | "it" | "we" | "they"
            | "me" | "him" | "her" | "us" | "them" | "my" | "your" | "his" | "its"
            | "our" | "their" | "this" | "that" | "these" | "those" | "to" | "of"
            | "for" | "in" | "on" | "at" | "by" | "with" | "and" | "or" | "but"
            | "if" | "then" | "than" | "so" | "as" | "be" | "am" | "is" | "are"
            | "was" | "were" | "been" | "have" | "has" | "had" | "do" | "does"
            | "did" | "can" | "could" | "will" | "would" | "shall" | "should"
            | "may" | "might" | "must"
    )
}

fn token_weight(token: &str) -> f64 {
    if stopword(token) { 0.28 } else { 1.0 }
}

fn levenshtein(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    if a.is_empty() {
        return b.chars().count();
    }
    if b.is_empty() {
        return a.chars().count();
    }

    let b_chars: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b_chars.len()).collect();
    let mut curr = vec![0; b_chars.len() + 1];

    for (i, ac) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, bc) in b_chars.iter().enumerate() {
            let cost = usize::from(ac != *bc);
            curr[j + 1] = (curr[j] + 1)
                .min(prev[j + 1] + 1)
                .min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_chars.len()]
}

fn root_equivalent(a: &str, b: &str) -> bool {
    let a_roots = roots(a);
    let b_roots = roots(b);

    a_roots.iter().any(|ar| {
        b_roots
            .iter()
            .any(|br| synonym_root(ar) == synonym_root(br))
    })
}

fn token_similarity(a: &str, b: &str) -> f64 {
    if root_equivalent(a, b) {
        return 1.0;
    }

    if a.len() < 4 || b.len() < 4 {
        return 0.0;
    }

    let distance = levenshtein(a, b);
    let max_len = a.chars().count().max(b.chars().count());
    if max_len == 0 {
        return 0.0;
    }

    let similarity = 1.0 - (distance as f64 / max_len as f64);
    if similarity >= 0.72 { similarity * 0.82 } else { 0.0 }
}

fn weighted_coverage(query: &[String], candidate: &[String]) -> f64 {
    let total: f64 = query.iter().map(|q| token_weight(q)).sum();
    if total == 0.0 {
        return 0.0;
    }

    let matched: f64 = query
        .iter()
        .map(|q| {
            let best = candidate
                .iter()
                .map(|c| token_similarity(q, c))
                .fold(0.0_f64, f64::max);
            token_weight(q) * best
        })
        .sum();

    matched / total
}

fn ordered_ratio(query: &[String], candidate: &[String]) -> f64 {
    if query.is_empty() {
        return 0.0;
    }

    let mut cursor = 0usize;
    let mut matched = 0usize;

    for q in query {
        while cursor < candidate.len() {
            if token_similarity(q, &candidate[cursor]) >= 0.95 {
                matched += 1;
                cursor += 1;
                break;
            }
            cursor += 1;
        }
    }

    matched as f64 / query.len() as f64
}

fn longest_contiguous_overlap(query: &[String], candidate: &[String]) -> usize {
    let mut best = 0usize;

    for qi in 0..query.len() {
        for ci in 0..candidate.len() {
            let mut length = 0usize;
            while qi + length < query.len()
                && ci + length < candidate.len()
                && token_similarity(&query[qi + length], &candidate[ci + length]) >= 0.95
            {
                length += 1;
            }
            best = best.max(length);
        }
    }

    best
}

fn score_text(query_raw: &str, candidate_raw: &str) -> (f64, bool) {
    let query_norm = normalize_text(query_raw);
    let candidate_norm = normalize_text(candidate_raw);

    if query_norm.is_empty() || candidate_norm.is_empty() {
        return (0.0, false);
    }

    let query_tokens = tokens(&query_norm);
    let candidate_tokens = tokens(&candidate_norm);

    let exact = if query_tokens.len() == 1 {
        candidate_tokens.iter().any(|token| token == &query_tokens[0])
    } else {
        candidate_norm.contains(&query_norm)
    };

    if exact {
        return (1.0, true);
    }

    let coverage = weighted_coverage(&query_tokens, &candidate_tokens);
    let order = ordered_ratio(&query_tokens, &candidate_tokens);
    let contiguous = longest_contiguous_overlap(&query_tokens, &candidate_tokens);

    let meaningful_query = query_tokens.iter().filter(|t| !stopword(t)).count().max(1);
    let meaningful_candidate = candidate_tokens
        .iter()
        .filter(|t| !stopword(t))
        .count()
        .max(1);
    let density = (meaningful_query as f64 / meaningful_candidate as f64).min(1.0);

    let mut score = coverage * 0.72 + order * 0.16 + density * 0.06;

    if contiguous >= 2 {
        let phrase_ratio = contiguous as f64 / query_tokens.len().max(1) as f64;
        let phrase_score = 0.54 + phrase_ratio * 0.36;
        score = score.max(phrase_score);
    }

    (score.min(0.99), false)
}

fn match_kind(score: f64, exact: bool) -> &'static str {
    if exact {
        "Exact"
    } else if score >= 0.82 {
        "Very close"
    } else if score >= 0.66 {
        "Close"
    } else {
        "Related"
    }
}

fn safe_collection_path(root: &Path, name: &str) -> Option<PathBuf> {
    if name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return None;
    }

    let path = root.join(name);
    if path.is_dir() { Some(path) } else { None }
}

#[tauri::command]
async fn search_subtitles(
    collection: Option<String>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SearchResult>, String> {
    tauri::async_runtime::spawn_blocking(move || search_subtitles_blocking(collection, query, limit))
        .await
        .map_err(|error| format!("Search worker failed: {error}"))?
}

fn search_subtitles_blocking(
    collection: Option<String>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SearchResult>, String> {
    let root = shows_dir()?;
    let query = query.trim();

    if query.is_empty() {
        return Ok(Vec::new());
    }

    let mut collections: Vec<(String, PathBuf)> = Vec::new();

    if let Some(name) = collection {
        let path = safe_collection_path(&root, &name)
            .ok_or_else(|| format!("Collection not found: {name}"))?;
        collections.push((name, path));
    } else {
        for entry in fs::read_dir(&root)
            .map_err(|e| format!("Could not read Shows folder: {e}"))?
        {
            let entry = entry.map_err(|e| format!("Could not read library item: {e}"))?;
            if entry.path().is_dir() {
                collections.push((
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.path(),
                ));
            }
        }
    }

    let threshold = if tokens(query).len() <= 1 { 0.58 } else { 0.46 };
    let mut results = Vec::new();
    let mut seen = HashSet::new();

    for (collection_name, folder) in collections {
        for file in subtitle_files(&folder) {
            let relative = file
                .strip_prefix(&folder)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            let file_name = file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("subtitle")
                .to_string();

            for cue in load_cues(&file) {
                let (score, exact) = score_text(query, &cue.text);
                if score < threshold {
                    continue;
                }

                let key = format!(
                    "{}|{}|{}|{}",
                    collection_name, relative, cue.start_ms, cue.text
                );
                if !seen.insert(key) {
                    continue;
                }

                results.push(SearchResult {
                    collection: collection_name.clone(),
                    file_name: file_name.clone(),
                    relative_path: relative.clone(),
                    start_ms: cue.start_ms,
                    end_ms: cue.end_ms,
                    text: cue.text,
                    score,
                    match_kind: match_kind(score, exact).to_string(),
                });
            }
        }
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.collection.cmp(&b.collection))
            .then_with(|| a.relative_path.cmp(&b.relative_path))
            .then_with(|| a.start_ms.cmp(&b.start_ms))
    });

    results.truncate(limit.unwrap_or(120).min(500));
    Ok(results)
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            library_root,
            scan_library,
            search_subtitles
        ])
        .run(tauri::generate_context!())
        .expect("error while running Subtitle Searcher");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_srt_and_keeps_timestamps() {
        let source = "1\n00:00:01,200 --> 00:00:03,400\nTake out the trash.\n\n2\n00:00:04,000 --> 00:00:06,000\nDone.";
        let cues = parse_subtitle_text(source);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start_ms, 1200);
        assert_eq!(cues[0].text, "Take out the trash.");
    }

    #[test]
    fn matches_irregular_phrasal_verb_order() {
        let (score, _) = score_text("take the trash out", "I already took out the garbage.");
        assert!(score >= 0.66, "score was {score}");
    }

    #[test]
    fn matches_inflection() {
        let (score, _) = score_text("wipe down the counter", "She wiped the counter down.");
        assert!(score >= 0.66, "score was {score}");
    }

    #[test]
    fn exact_phrase_wins() {
        let (score, exact) = score_text(
            "do the laundry",
            "You were supposed to do the laundry yesterday.",
        );
        assert!(exact);
        assert_eq!(score, 1.0);
    }
}