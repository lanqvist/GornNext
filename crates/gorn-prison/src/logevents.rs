//! События лога игры при кадре — коротко, для журнала фактов (этап 8):
//! что нажато, какие окна открылись и закрылись, какой звук сыграл, какой день.
//! По ним видно, что произошло между кадрами, — ход и окно не угадываются
//! по картинке. Предупреждения таблиц и прочий шум отбрасываются.

/// Строки лога → метки: `click:Btn_…`, `S16BoardRelicPop Run`, `sfx:UI_S16_Click_Eat`, `day:5/20`.
pub fn compact(lines: &[String]) -> Vec<String> {
    let mut out = vec![];
    for l in lines {
        if l.contains("LogDataTable") {
            continue;
        }
        if let Some(rest) = after(l, "normal-click ") {
            out.push(format!("click:{}", word(rest)));
        } else if let Some(rest) = after(l, "AudioEnum") {
            let name = word(rest.trim_start_matches([' ', '=']));
            if name.starts_with("UI_S16") {
                out.push(format!("sfx:{name}"));
            }
        } else if let Some(rest) = after(l, "check cur=") {
            let cur = word(rest);
            let max = after(rest, "max=").map(word).unwrap_or("?");
            out.push(format!("day:{cur}/{max}"));
        } else if let Some(rest) = after(l, "[Game] S16") {
            let name = word(rest);
            let state = word(rest[name.len()..].trim_start());
            if matches!(state, "Run" | "Destory") {
                out.push(format!("S16{name} {state}"));
            }
        }
    }
    out
}

fn after<'a>(s: &'a str, key: &str) -> Option<&'a str> {
    s.find(key).map(|i| &s[i + key.len()..])
}

fn word(s: &str) -> &str {
    s.split(|c: char| c.is_whitespace()).next().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::compact;

    #[test]
    fn lines_to_events() {
        let lines: Vec<String> = [
            "[2026.10.21-19.32.19:251][323]TLGame: Display: [Game] UIButton@ normal-click Btn_S16Board_RouteCell",
            "[2026.10.21-19.32.19:251][323]LogDataTable: Warning: UDataTable::FindRow : 'AudioMgr' requested row 'X'",
            "[2026.10.21-19.32.19:567][395]TLLua: Display: [Game] S16BoardRelicPop Run",
            "[2026.10.21-19.32.20:000][1]TLLua: Display: [Game] AudioMgr@PlayAudio  AudioEnum  == UI_S16_Click_Eat",
            "[2026.10.21-19.32.17:951][ 17]TLLua: Display: [Game] [S16LastDay] check cur=5 max=20 played=false",
            "[2026.10.21-19.32.21:000][1]TLLua: Display: [Game] S16BoardColoredPop Destory",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            compact(&lines),
            ["click:Btn_S16Board_RouteCell", "S16BoardRelicPop Run", "sfx:UI_S16_Click_Eat", "day:5/20", "S16BoardColoredPop Destory"]
        );
    }
}
