//! Map MCP `clientInfo.name` to a small known-host table, else an identicon.
//! Names are self-reported; this is display only, not auth.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OccupantFace {
    /// Stable key: `cursor` | `grok` | … | `unknown`
    pub key: String,
    /// Human label shown on the seat.
    pub label: String,
    pub initials: String,
    pub hue: u16,
    pub known: bool,
}

const KNOWN: &[(&[&str], &str, &str, u16)] = &[
    (&["cursor"], "cursor", "Cursor", 220),
    (&["grokbuild", "grok", "xiaigrok"], "grok", "Grok", 30),
    (&["openaicodex", "codex"], "codex", "Codex", 160),
    (&["kimicode", "moonshot", "kimi"], "kimi", "Kimi", 280),
    (
        &["googleantigravity", "antigravity"],
        "antigravity",
        "Antigravity",
        200,
    ),
    (
        &["claudecode", "claudedesktop", "anthropic", "claude"],
        "claude",
        "Claude",
        18,
    ),
    (&["windsurf", "codeium"], "windsurf", "Windsurf", 190),
    (
        &["visualstudiocode", "githubcopilot", "vscode", "copilot"],
        "vscode",
        "VS Code",
        210,
    ),
    (&["trae"], "trae", "Trae", 250),
    (&["cline"], "cline", "Cline", 145),
    (&["continue"], "continue", "Continue", 265),
    (&["zed"], "zed", "Zed", 45),
];

pub fn recognize(raw_name: &str, title: Option<&str>) -> OccupantFace {
    let name = raw_name.trim();
    let title = title.unwrap_or("").trim();
    let blob = format!("{name} {title}");
    let compact: String = blob
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();

    for (needles, key, label, hue) in KNOWN {
        if needles.iter().any(|n| compact.contains(n)) {
            return OccupantFace {
                key: (*key).to_string(),
                label: (*label).to_string(),
                initials: initials_from(label),
                hue: *hue,
                known: true,
            };
        }
    }

    let label = if !title.is_empty() {
        title.to_string()
    } else if !name.is_empty() {
        name.to_string()
    } else {
        "MCP".to_string()
    };
    OccupantFace {
        key: "unknown".into(),
        label: label.clone(),
        initials: initials_from(&label),
        hue: hue_of(&compact),
        known: false,
    }
}

fn initials_from(s: &str) -> String {
    let parts: Vec<char> = s
        .split(|c: char| !c.is_alphanumeric())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.chars().next())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if parts.len() >= 2 {
        return format!("{}{}", parts[0], parts[1]);
    }
    let letters: String = s
        .chars()
        .filter(|c| c.is_alphanumeric())
        .take(2)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if letters.is_empty() {
        "MCP".into()
    } else {
        letters
    }
}

fn hue_of(s: &str) -> u16 {
    let mut h: u32 = 2166136261;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    (h % 360) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_aliases() {
        assert!(recognize("cursor", None).known);
        assert_eq!(recognize("Cursor", None).key, "cursor");
        assert_eq!(recognize("grok-build", None).key, "grok");
        assert_eq!(recognize("openai-codex", None).key, "codex");
        assert_eq!(recognize("kimi-code", None).key, "kimi");
        assert_eq!(recognize("antigravity", None).key, "antigravity");
        assert_eq!(recognize("claude-code", None).key, "claude");
    }

    #[test]
    fn unknown_gets_identicon() {
        let f = recognize("seat-check", None);
        assert!(!f.known);
        assert_eq!(f.key, "unknown");
        assert_eq!(f.initials, "SC");
        assert_eq!(f.label, "seat-check");
    }

    #[test]
    fn anti_alone_is_not_antigravity() {
        let f = recognize("anthropic-helper", None);
        assert_eq!(f.key, "claude");
        let g = recognize("my-anti-plugin", None);
        assert!(!g.known, "bare 'anti' must not map to Antigravity");
    }
}
