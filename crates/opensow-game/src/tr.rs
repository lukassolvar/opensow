//! The port's own translations of the skirmish texts (main menu row, lobby panel). The game's language files have no SKIRMISH
//! (it is not in the original), so English, Slovak, Czech and both Chinese scripts are written here, in the register of the
//! original strings (lower-case menu row in Slovak / Czech). Other languages fall back to English (they are not offered anyway).

use crate::sim::Difficulty;
use opensow_formats::lang::Language;

/// Index into the five-language tables below: English, Slovak, Czech, Chinese (simplified), Chinese (traditional).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tr(usize);

impl Tr {
    pub fn new(language: Language) -> Tr {
        Tr(match language {
            Language::Slovak => 1,
            Language::Czech => 2,
            Language::ChineseSimplified => 3,
            Language::ChineseTraditional => 4,
            _ => 0,
        })
    }

    fn pick(self, t: [&'static str; 5]) -> &'static str {
        t[self.0]
    }

    /// The main menu row.
    pub fn menu_row(self) -> &'static str {
        self.pick(["Skirmish", "potyčka", "potyčka", "遭遇战", "遭遇戰"])
    }

    /// The lobby's heading.
    pub fn title(self) -> &'static str {
        self.pick(["SKIRMISH", "POTYČKA", "POTYČKA", "遭遇战", "遭遇戰"])
    }

    pub fn star_none(self) -> &'static str {
        self.pick(["Star: not earned yet", "Hviezda: ešte nezískaná", "Hvězda: zatím nezískána", "星章：尚未获得", "星章：尚未獲得"])
    }

    /// "Star: silver (Normal)".
    pub fn star_line(self, d: Difficulty) -> String {
        let head = self.pick(["Star", "Hviezda", "Hvězda", "星章", "星章"]);
        let colon = if self.0 >= 3 { "：" } else { ": " };
        format!("{head}{colon}{} ({})", self.star_name(d), self.difficulty(d))
    }

    pub fn star_name(self, d: Difficulty) -> &'static str {
        let rank = Difficulty::ALL.iter().position(|&x| x == d).unwrap_or(1);
        [
            ["bronze", "silver", "gold", "ruby"],
            ["bronzová", "strieborná", "zlatá", "rubínová"],
            ["bronzová", "stříbrná", "zlatá", "rubínová"],
            ["铜", "银", "金", "红宝石"],
            ["銅", "銀", "金", "紅寶石"],
        ][self.0][rank]
    }

    pub fn setup_default(self) -> &'static str {
        self.pick([
            "Default setup: a win earns a star",
            "Predvolené nastavenie: výhra získa hviezdu",
            "Výchozí nastavení: výhra získá hvězdu",
            "默认设置：获胜可得星章",
            "預設設定：獲勝可得星章",
        ])
    }

    pub fn setup_changed(self) -> &'static str {
        self.pick(["Setup changed: no star", "Nastavenie zmenené: bez hviezdy", "Nastavení změněno: bez hvězdy", "设置已更改：无星章", "設定已更改：無星章"])
    }

    pub fn difficulty(self, d: Difficulty) -> &'static str {
        let rank = Difficulty::ALL.iter().position(|&x| x == d).unwrap_or(1);
        [
            ["Easy", "Normal", "Hard", "Brutal"],
            ["Ľahká", "Normálna", "Ťažká", "Brutálna"],
            ["Lehká", "Normální", "Těžká", "Brutální"],
            ["简单", "普通", "困难", "残酷"],
            ["簡單", "普通", "困難", "殘酷"],
        ][self.0][rank]
    }

    /// The setup screen's line under its title.
    pub fn subtitle(self) -> &'static str {
        self.pick([
            "Battle the computer on any multiplayer map",
            "Bojuj proti počítaču na ľubovoľnej mape",
            "Bojuj proti počítači na libovolné mapě",
            "在任意多人地图上与电脑对战",
            "在任意多人地圖上與電腦對戰",
        ])
    }

    pub fn maps(self) -> &'static str {
        self.pick(["MAPS", "MAPY", "MAPY", "地图", "地圖"])
    }

    pub fn players(self) -> &'static str {
        self.pick(["PLAYERS", "HRÁČI", "HRÁČI", "玩家", "玩家"])
    }

    pub fn resources(self) -> &'static str {
        self.pick(["STARTING RESOURCES", "POČIATOČNÉ ZDROJE", "POČÁTEČNÍ ZDROJE", "初始资源", "初始資源"])
    }

    pub fn air_forces(self) -> &'static str {
        self.pick(["AIR FORCES", "LETECTVO", "LETECTVO", "空军", "空軍"])
    }

    pub fn turrets(self) -> &'static str {
        self.pick(["TURRETS ALLOWED", "POVOLENÉ VEŽE", "POVOLENÉ VĚŽE", "可用炮塔", "可用砲塔"])
    }

    pub fn you(self) -> &'static str {
        self.pick(["YOU", "TY", "TY", "你", "你"])
    }

    pub fn computer(self) -> &'static str {
        self.pick(["COMPUTER", "POČÍTAČ", "POČÍTAČ", "电脑", "電腦"])
    }

    pub fn side(self, green: bool) -> &'static str {
        self.pick(if green { ["Green side", "Zelená strana", "Zelená strana", "绿队", "綠隊"] } else { ["Blue side", "Modrá strana", "Modrá strana", "蓝队", "藍隊"] })
    }

    /// The short column heads over the two sides' values.
    pub fn side_short(self, green: bool) -> &'static str {
        self.pick(if green { ["GREEN", "ZELENÁ", "ZELENÁ", "绿队", "綠隊"] } else { ["BLUE", "MODRÁ", "MODRÁ", "蓝队", "藍隊"] })
    }

    pub fn swap(self) -> &'static str {
        self.pick(["Swap sides", "Vymeniť strany", "Vyměnit strany", "交换阵营", "交換陣營"])
    }

    pub fn reset(self) -> &'static str {
        self.pick(["Reset defaults", "Pôvodné nastavenie", "Původní nastavení", "恢复默认", "恢復預設"])
    }

    pub fn start(self) -> &'static str {
        self.pick(["START", "ŠTART", "START", "开始", "開始"])
    }

    pub fn back(self) -> &'static str {
        self.pick(["BACK", "SPÄŤ", "ZPĚT", "返回", "返回"])
    }

    /// The map facts under the picture: labels, then the value lines.
    pub fn fact_labels(self) -> [&'static str; 4] {
        match self.0 {
            1 => ["Veľkosť", "Budovy", "Zlaté bane", "Terén"],
            2 => ["Velikost", "Budovy", "Zlaté doly", "Terén"],
            3 => ["大小", "建筑", "金矿", "地形"],
            4 => ["大小", "建築", "金礦", "地形"],
            _ => ["Size", "Buildings", "Gold mines", "Terrain"],
        }
    }

    pub fn buildings(self, all: usize, neutral: usize) -> String {
        match self.0 {
            1 => format!("{all}  ({neutral} neutrálnych)"),
            2 => format!("{all}  ({neutral} neutrálních)"),
            3 | 4 => format!("{all}（{neutral} 中立）"),
            _ => format!("{all}  ({neutral} neutral)"),
        }
    }

    pub fn mission(self, n: i32) -> String {
        match self.0 {
            1 => format!("Misia {n}"),
            2 => format!("Mise {n}"),
            3 => format!("任务 {n}"),
            4 => format!("任務 {n}"),
            _ => format!("Mission {n}"),
        }
    }

    /// The computer's handicap, one line each: income, production.
    pub fn income(self, income_pct: u32, production_pct: u32) -> [String; 2] {
        match self.0 {
            1 => [format!("príjem {income_pct}%"), format!("výroba {production_pct}%")],
            2 => [format!("příjem {income_pct}%"), format!("výroba {production_pct}%")],
            3 => [format!("收入 {income_pct}%"), format!("生产 {production_pct}%")],
            4 => [format!("收入 {income_pct}%"), format!("生產 {production_pct}%")],
            _ => [format!("income {income_pct}%"), format!("production {production_pct}%")],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Language; 5] = [Language::English, Language::Slovak, Language::Czech, Language::ChineseSimplified, Language::ChineseTraditional];

    #[test]
    fn english_is_the_original_wording() {
        let t = Tr::new(Language::English);
        assert_eq!(t.star_line(Difficulty::Normal), "Star: silver (Normal)");
        assert_eq!(t.income(100, 80), ["income 100%", "production 80%"]);
        assert_eq!(Tr::new(Language::Polish), t, "unsupported languages read English");
    }

    #[test]
    fn every_language_has_every_text() {
        for l in ALL {
            let t = Tr::new(l);
            for s in [t.menu_row(), t.title(), t.star_none(), t.setup_default(), t.setup_changed(), t.subtitle(), t.swap(), t.reset(), t.start(), t.back()] {
                assert!(!s.is_empty(), "{l:?}");
            }
            for d in Difficulty::ALL {
                assert!(!t.difficulty(d).is_empty() && !t.star_name(d).is_empty());
            }
            assert!(!t.side(true).is_empty() && !t.income(1, 2)[1].is_empty() && !t.buildings(3, 1).is_empty());
        }
        // all five tables differ from each other in the heading
        let titles: std::collections::HashSet<_> = ALL.iter().map(|&l| Tr::new(l).title()).collect();
        assert!(titles.len() >= 4);
    }
}
