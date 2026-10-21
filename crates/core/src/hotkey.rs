//! Hotkey description, parsing from / formatting to the config string (e.g. `Ctrl+Pause`).

use std::fmt;
use std::str::FromStr;

pub const VK_PAUSE: u32 = 0x13;

/// Named non-alphanumeric keys: (config name, virtual-key code).
const NAMED_KEYS: &[(&str, u32)] = &[
    ("Pause", VK_PAUSE),
    ("ScrollLock", 0x91),
    ("Insert", 0x2D),
    ("Delete", 0x2E),
    ("Home", 0x24),
    ("End", 0x23),
    ("PageUp", 0x21),
    ("PageDown", 0x22),
    ("Space", 0x20),
    ("Tab", 0x09),
    ("Escape", 0x1B),
    ("NumLock", 0x90),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub vk: u32,
}

impl Default for Hotkey {
    fn default() -> Self {
        Hotkey { ctrl: false, alt: false, shift: false, win: false, vk: VK_PAUSE }
    }
}

/// Returns the config name of a virtual key, if the key can be used in a hotkey.
pub fn vk_name(vk: u32) -> Option<String> {
    if let Some((name, _)) = NAMED_KEYS.iter().find(|(_, v)| *v == vk) {
        return Some((*name).to_string());
    }
    match vk {
        0x30..=0x39 | 0x41..=0x5A => Some(char::from_u32(vk)?.to_string()),
        0x70..=0x87 => Some(format!("F{}", vk - 0x6F)),
        _ => None,
    }
}

fn name_vk(name: &str) -> Option<u32> {
    if let Some((_, vk)) = NAMED_KEYS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
        return Some(*vk);
    }
    if name.eq_ignore_ascii_case("Break") {
        return Some(VK_PAUSE);
    }
    let upper = name.to_ascii_uppercase();
    let mut chars = upper.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_uppercase() || c.is_ascii_digit() => Some(c as u32),
        (Some('F'), Some(_)) => {
            let n: u32 = upper[1..].parse().ok()?;
            (1..=24).contains(&n).then_some(0x6F + n)
        }
        _ => None,
    }
}

impl FromStr for Hotkey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut hk = Hotkey { vk: 0, ..Hotkey::default() };
        let mut key_seen = false;
        for part in s.split('+').map(str::trim) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => hk.ctrl = true,
                "alt" => hk.alt = true,
                "shift" => hk.shift = true,
                "win" | "super" => hk.win = true,
                _ => {
                    if key_seen {
                        return Err(format!("more than one key in hotkey '{s}'"));
                    }
                    hk.vk = name_vk(part).ok_or_else(|| format!("unknown key '{part}'"))?;
                    key_seen = true;
                }
            }
        }
        if key_seen {
            Ok(hk)
        } else {
            Err(format!("no key in hotkey '{s}'"))
        }
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (on, name) in [(self.ctrl, "Ctrl"), (self.alt, "Alt"), (self.shift, "Shift"), (self.win, "Win")] {
            if on {
                write!(f, "{name}+")?;
            }
        }
        write!(f, "{}", vk_name(self.vk).unwrap_or_else(|| format!("0x{:02X}", self.vk)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_and_combined() {
        assert_eq!("Pause".parse::<Hotkey>().unwrap(), Hotkey::default());
        let hk: Hotkey = "ctrl + shift+F9".parse().unwrap();
        assert!(hk.ctrl && hk.shift && !hk.alt && !hk.win);
        assert_eq!(hk.vk, 0x78);
        assert_eq!(hk.to_string(), "Ctrl+Shift+F9");
    }

    #[test]
    fn rejects_garbage() {
        assert!("".parse::<Hotkey>().is_err());
        assert!("Ctrl".parse::<Hotkey>().is_err());
        assert!("A+B".parse::<Hotkey>().is_err());
        assert!("Foo".parse::<Hotkey>().is_err());
    }

    #[test]
    fn roundtrip_names() {
        for vk in [VK_PAUSE, 0x41, 0x35, 0x70, 0x2D, 0x91] {
            let name = vk_name(vk).unwrap();
            assert_eq!(name.parse::<Hotkey>().unwrap().vk, vk, "{name}");
        }
    }
}
