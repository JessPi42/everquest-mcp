use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedEvent {
    pub event_type: String,
    pub payload: serde_json::Value,
    pub zone: Option<String>,
    pub character_name: Option<String>,
    pub timestamp: String,
}

static RE_LOOT: OnceLock<Regex> = OnceLock::new();
static RE_KILL_YOU: OnceLock<Regex> = OnceLock::new();
static RE_KILL_SLAIN: OnceLock<Regex> = OnceLock::new();
static RE_ZONE: OnceLock<Regex> = OnceLock::new();
static RE_SPELL_CAST: OnceLock<Regex> = OnceLock::new();
static RE_SPELL_FIZZLE: OnceLock<Regex> = OnceLock::new();
static RE_SPELL_RESIST: OnceLock<Regex> = OnceLock::new();
static RE_SKILLUP: OnceLock<Regex> = OnceLock::new();
static RE_NPC_SAYS: OnceLock<Regex> = OnceLock::new();
static RE_PLAYER_TASK_COMPLETE: OnceLock<Regex> = OnceLock::new();
static RE_QUEST_TASK: OnceLock<Regex> = OnceLock::new();
static RE_QUEST_REWARD_GIVEN: OnceLock<Regex> = OnceLock::new();
static RE_QUEST_REWARD_RECEIVE: OnceLock<Regex> = OnceLock::new();
static RE_QUEST_XP: OnceLock<Regex> = OnceLock::new();
static RE_WHO_PLAYER: OnceLock<Regex> = OnceLock::new();
static RE_WHO_COUNT: OnceLock<Regex> = OnceLock::new();
static RE_TIMESTAMP: OnceLock<Regex> = OnceLock::new();

fn re_loot() -> &'static Regex {
    RE_LOOT.get_or_init(|| Regex::new(r"You receive (.+?) from (.+?)\.").unwrap())
}

fn re_kill_you() -> &'static Regex {
    RE_KILL_YOU.get_or_init(|| Regex::new(r"You have slain (.+?)!").unwrap())
}

fn re_kill_slain() -> &'static Regex {
    RE_KILL_SLAIN.get_or_init(|| Regex::new(r"(.+?) has been slain by (.+?)!").unwrap())
}

fn re_zone() -> &'static Regex {
    RE_ZONE.get_or_init(|| Regex::new(r"You have entered (.+?)\.").unwrap())
}

fn re_spell_cast() -> &'static Regex {
    RE_SPELL_CAST.get_or_init(|| Regex::new(r"(.+?) begins to cast a spell\.").unwrap())
}

fn re_spell_fizzle() -> &'static Regex {
    RE_SPELL_FIZZLE.get_or_init(|| Regex::new(r"Your (.+?) spell fizzles!").unwrap())
}

fn re_spell_resist() -> &'static Regex {
    RE_SPELL_RESIST.get_or_init(|| Regex::new(r"(.+?) resisted your (.+?) spell!").unwrap())
}

fn re_skillup() -> &'static Regex {
    RE_SKILLUP.get_or_init(|| {
        Regex::new(r"You have become better at (.+?)! \((\d+)\)").unwrap()
    })
}

fn re_npc_says() -> &'static Regex {
    RE_NPC_SAYS.get_or_init(|| Regex::new(r"(.+?) says, '(.+?)'").unwrap())
}

/// "You say, 'Task Complete'" — classic EQ quest turn-in trigger phrase.
fn re_player_task_complete() -> &'static Regex {
    RE_PLAYER_TASK_COMPLETE.get_or_init(|| {
        Regex::new(r"(?i)You say,? '(?:Task Complete|Hail)'").unwrap()
    })
}

/// Task-system quest completion: Task 'Name' completed!
fn re_quest_task() -> &'static Regex {
    RE_QUEST_TASK.get_or_init(|| Regex::new(r"Task '(.+?)' completed!").unwrap())
}

/// Admin-gifted or turn-in reward: "You have been given: Item Name."
fn re_quest_reward_given() -> &'static Regex {
    RE_QUEST_REWARD_GIVEN.get_or_init(|| Regex::new(r"You have been given: (.+?)\.").unwrap())
}

/// Classic turn-in reward: "You receive Item Name as a reward."
fn re_quest_reward_receive() -> &'static Regex {
    RE_QUEST_REWARD_RECEIVE.get_or_init(|| {
        Regex::new(r"You receive (.+?) as a reward\.").unwrap()
    })
}

/// Experience granted on quest completion: "You gain experience!" or "You gain X experience points!"
fn re_quest_xp() -> &'static Regex {
    RE_QUEST_XP.get_or_init(|| {
        Regex::new(r"You gain (?:(\d+) )?experience(?: points)?!").unwrap()
    })
}

/// /who output: "[50 Warrior] PlayerName (Race) <Guild>" — guild and race are optional
fn re_who_player() -> &'static Regex {
    RE_WHO_PLAYER.get_or_init(|| {
        Regex::new(r"^\[(\d+) ([^\]]+)\] (\S+)(?: \(([^)]+)\))?(?: <(.+?)>)?$").unwrap()
    })
}

/// /who zone summary: "There are N players in Zone." or "There is 1 player in Zone."
fn re_who_count() -> &'static Regex {
    RE_WHO_COUNT.get_or_init(|| {
        Regex::new(r"There (?:are (\d+) players|is (\d+) player) in (.+?)\.").unwrap()
    })
}

fn re_timestamp() -> &'static Regex {
    RE_TIMESTAMP.get_or_init(|| Regex::new(r"^\[([^\]]+)\] (.+)$").unwrap())
}

/// Extract meaningful keywords from NPC dialogue text for quest-context indexing.
/// Filters out common English stop words and words shorter than 4 chars.
fn extract_keywords(text: &str) -> Vec<String> {
    const STOP_WORDS: &[&str] = &[
        "this", "that", "with", "from", "have", "will", "your", "there",
        "they", "what", "when", "which", "then", "than", "been", "were",
        "here", "also", "into", "some", "more", "very", "just", "like",
        "would", "could", "should", "about", "after", "before", "their",
        "these", "those", "shall", "upon", "unto", "thee", "thou", "hast",
        "dost", "know", "need", "must", "come", "back", "many", "much",
    ];
    let mut seen = std::collections::HashSet::new();
    let keywords: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| w.len() >= 4)
        .filter(|w| !STOP_WORDS.contains(&w.to_lowercase().as_str()))
        .filter(|w| seen.insert(w.to_lowercase()))
        .take(12)
        .map(|w| w.to_string())
        .collect();
    keywords
}

/// Known player/system speakers to exclude from NPC dialogue capture.
fn is_excluded_speaker(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "you"
        || lower == "yourself"
        || lower.contains("guild")
        || lower.starts_with('(')
        || lower == "a mysterious voice"
}

pub struct LogParser {
    pub current_zone: Option<String>,
    pub character_name: Option<String>,
    /// Accumulates pending quest context across log lines (task name seen before reward).
    pending_quest_name: Option<String>,
}

impl LogParser {
    pub fn new(character_name: Option<String>) -> Self {
        Self {
            current_zone: None,
            character_name,
            pending_quest_name: None,
        }
    }

    pub fn parse_line(&mut self, line: &str) -> Option<ParsedEvent> {
        let caps = re_timestamp().captures(line)?;
        let timestamp = caps.get(1)?.as_str().to_string();
        let body = caps.get(2)?.as_str();

        // Zone entry
        if let Some(c) = re_zone().captures(body) {
            let zone = c.get(1)?.as_str().to_string();
            self.current_zone = Some(zone.clone());
            return Some(ParsedEvent {
                event_type: "zone".into(),
                payload: serde_json::json!({ "zone": zone }),
                zone: Some(zone),
                character_name: self.character_name.clone(),
                timestamp,
            });
        }

        // Loot
        if let Some(c) = re_loot().captures(body) {
            let item = c.get(1)?.as_str().to_string();
            let npc = c.get(2)?.as_str().to_string();
            return Some(ParsedEvent {
                event_type: "loot".into(),
                payload: serde_json::json!({ "item": item, "npc": npc }),
                zone: self.current_zone.clone(),
                character_name: self.character_name.clone(),
                timestamp,
            });
        }

        // Kill - you slain
        if let Some(c) = re_kill_you().captures(body) {
            let npc = c.get(1)?.as_str().to_string();
            return Some(ParsedEvent {
                event_type: "kill".into(),
                payload: serde_json::json!({ "npc": npc, "slayer": "You" }),
                zone: self.current_zone.clone(),
                character_name: self.character_name.clone(),
                timestamp,
            });
        }

        // Kill - slain by
        if let Some(c) = re_kill_slain().captures(body) {
            let npc = c.get(1)?.as_str().to_string();
            let slayer = c.get(2)?.as_str().to_string();
            return Some(ParsedEvent {
                event_type: "kill".into(),
                payload: serde_json::json!({ "npc": npc, "slayer": slayer }),
                zone: self.current_zone.clone(),
                character_name: self.character_name.clone(),
                timestamp,
            });
        }

        // Task-system quest completion: sets pending_quest_name for reward lines that follow
        if let Some(c) = re_quest_task().captures(body) {
            let quest = c.get(1)?.as_str().to_string();
            self.pending_quest_name = Some(quest.clone());
            return Some(ParsedEvent {
                event_type: "quest".into(),
                payload: serde_json::json!({ "quest": quest, "subtype": "completed" }),
                zone: self.current_zone.clone(),
                character_name: self.character_name.clone(),
                timestamp,
            });
        }

        // Quest reward — "You have been given: Item."
        if let Some(c) = re_quest_reward_given().captures(body) {
            let item = c.get(1)?.as_str().to_string();
            let quest = self.pending_quest_name.clone();
            return Some(ParsedEvent {
                event_type: "quest".into(),
                payload: serde_json::json!({
                    "subtype": "reward",
                    "item": item,
                    "quest": quest,
                }),
                zone: self.current_zone.clone(),
                character_name: self.character_name.clone(),
                timestamp,
            });
        }

        // Quest reward — "You receive Item as a reward." (classic turn-in)
        if let Some(c) = re_quest_reward_receive().captures(body) {
            let item = c.get(1)?.as_str().to_string();
            let quest = self.pending_quest_name.take();
            return Some(ParsedEvent {
                event_type: "quest".into(),
                payload: serde_json::json!({
                    "subtype": "reward",
                    "item": item,
                    "quest": quest,
                }),
                zone: self.current_zone.clone(),
                character_name: self.character_name.clone(),
                timestamp,
            });
        }

        // Experience gain (quest completion XP)
        if let Some(c) = re_quest_xp().captures(body) {
            let xp: Option<u64> = c.get(1).and_then(|m| m.as_str().parse().ok());
            let quest = self.pending_quest_name.take();
            return Some(ParsedEvent {
                event_type: "quest".into(),
                payload: serde_json::json!({
                    "subtype": "xp",
                    "xp": xp,
                    "quest": quest,
                }),
                zone: self.current_zone.clone(),
                character_name: self.character_name.clone(),
                timestamp,
            });
        }

        // /who player entry: [level class] Name (Race) <Guild>
        if let Some(c) = re_who_player().captures(body) {
            let level: u32 = c.get(1)?.as_str().parse().unwrap_or(0);
            let class = c.get(2)?.as_str().to_string();
            let name = c.get(3)?.as_str().to_string();
            let race = c.get(4).map(|m| m.as_str().to_string());
            let guild = c.get(5).map(|m| m.as_str().to_string());
            return Some(ParsedEvent {
                event_type: "who".into(),
                payload: serde_json::json!({
                    "name": name,
                    "level": level,
                    "class": class,
                    "race": race,
                    "guild": guild,
                }),
                zone: self.current_zone.clone(),
                character_name: self.character_name.clone(),
                timestamp,
            });
        }

        // /who zone count summary
        if let Some(c) = re_who_count().captures(body) {
            let count: u32 = c.get(1)
                .or_else(|| c.get(2))
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            let zone = c.get(3)?.as_str().to_string();
            return Some(ParsedEvent {
                event_type: "who".into(),
                payload: serde_json::json!({ "subtype": "count", "count": count, "zone": zone }),
                zone: Some(zone),
                character_name: self.character_name.clone(),
                timestamp,
            });
        }

        // Spell fizzle
        if let Some(c) = re_spell_fizzle().captures(body) {
            let spell = c.get(1)?.as_str().to_string();
            return Some(ParsedEvent {
                event_type: "spell".into(),
                payload: serde_json::json!({ "spell": spell, "result": "fizzle" }),
                zone: self.current_zone.clone(),
                character_name: self.character_name.clone(),
                timestamp,
            });
        }

        // Spell resist
        if let Some(c) = re_spell_resist().captures(body) {
            let target = c.get(1)?.as_str().to_string();
            let spell = c.get(2)?.as_str().to_string();
            return Some(ParsedEvent {
                event_type: "spell".into(),
                payload: serde_json::json!({ "spell": spell, "target": target, "result": "resist" }),
                zone: self.current_zone.clone(),
                character_name: self.character_name.clone(),
                timestamp,
            });
        }

        // Spell cast
        if let Some(c) = re_spell_cast().captures(body) {
            let caster = c.get(1)?.as_str().to_string();
            return Some(ParsedEvent {
                event_type: "spell".into(),
                payload: serde_json::json!({ "caster": caster, "result": "cast_begin" }),
                zone: self.current_zone.clone(),
                character_name: self.character_name.clone(),
                timestamp,
            });
        }

        // Skill up
        if let Some(c) = re_skillup().captures(body) {
            let skill = c.get(1)?.as_str().to_string();
            let level: u32 = c.get(2)?.as_str().parse().ok()?;
            return Some(ParsedEvent {
                event_type: "skillup".into(),
                payload: serde_json::json!({ "skill": skill, "level": level }),
                zone: self.current_zone.clone(),
                character_name: self.character_name.clone(),
                timestamp,
            });
        }

        // Player says "Task Complete" (classic EQ turn-in trigger)
        if re_player_task_complete().is_match(body) {
            return Some(ParsedEvent {
                event_type: "quest".into(),
                payload: serde_json::json!({
                    "subtype": "say_complete",
                    "quest": self.pending_quest_name.clone(),
                }),
                zone: self.current_zone.clone(),
                character_name: self.character_name.clone(),
                timestamp,
            });
        }

        // NPC dialogue (filter out player/guild/system channels)
        if let Some(c) = re_npc_says().captures(body) {
            let speaker = c.get(1)?.as_str();
            let text = c.get(2)?.as_str().to_string();
            if !is_excluded_speaker(speaker) {
                let keywords = extract_keywords(&text);
                return Some(ParsedEvent {
                    event_type: "dialogue".into(),
                    payload: serde_json::json!({
                        "npc": speaker,
                        "text": text,
                        "keywords": keywords,
                    }),
                    zone: self.current_zone.clone(),
                    character_name: self.character_name.clone(),
                    timestamp,
                });
            }
        }

        None
    }
}
