//! The port's own translations of the skirmish texts (main menu row, lobby panel). The game's language files have no SKIRMISH
//! (it is not in the original), so English, Slovak, Czech and both Chinese scripts are written here, in the register of the
//! original strings (capitalised menu rows, like `LangFile::with_supplement` makes the Slovak / Czech ones). Other languages fall back to English (they are not offered anyway).

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
        self.pick(["Skirmish", "Potyčka", "Potyčka", "遭遇战", "遭遇戰"])
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
        self.pick(if green {
            ["Green side", "Zelená strana", "Zelená strana", "绿队", "綠隊"]
        } else {
            ["Blue side", "Modrá strana", "Modrá strana", "蓝队", "藍隊"]
        })
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

    // ---- custom maps (skirmish screen's Custom tab) and the map editor, docs/map-editor-ingame.md ----

    pub fn tab_original(self) -> &'static str {
        self.pick(["Original", "Pôvodné", "Původní", "原版", "原版"])
    }

    pub fn tab_custom(self) -> &'static str {
        self.pick(["Custom", "Vlastné", "Vlastní", "自定义", "自訂"])
    }

    pub fn open_folder(self) -> &'static str {
        self.pick(["Open folder", "Otvoriť priečinok", "Otevřít složku", "打开文件夹", "開啟資料夾"])
    }

    pub fn edit_map(self) -> &'static str {
        self.pick(["Edit", "Upraviť", "Upravit", "编辑", "編輯"])
    }

    pub fn no_custom_maps(self) -> &'static str {
        self.pick([
            "No custom maps yet. Make one in the map editor, or copy map files into the maps folder.",
            "Zatiaľ žiadne vlastné mapy. Vytvorte ich v editore máp alebo skopírujte súbory máp do priečinka máp.",
            "Zatím žádné vlastní mapy. Vytvořte je v editoru map nebo zkopírujte soubory map do složky map.",
            "还没有自定义地图。请在地图编辑器中制作，或将地图文件复制到地图文件夹。",
            "還沒有自訂地圖。請在地圖編輯器中製作，或將地圖檔案複製到地圖資料夾。",
        ])
    }

    pub fn no_custom_short(self) -> &'static str {
        self.pick(["No custom maps", "Žiadne vlastné mapy", "Žádné vlastní mapy", "没有自定义地图", "沒有自訂地圖"])
    }

    /// Why a custom map cannot be played.
    pub fn blocker(self, b: &crate::custom_maps::Blocker) -> String {
        match b {
            // the parser's detail stays in the log (`custom_maps::list` prints it), the page says what it means
            crate::custom_maps::Blocker::Broken(_) => self
                .pick([
                    "This map file cannot be read: it is damaged or not a map.",
                    "Súbor mapy sa nedá prečítať: je poškodený alebo nie je mapou.",
                    "Soubor mapy nelze přečíst: je poškozený nebo to není mapa.",
                    "无法读取此地图文件：文件已损坏或不是地图。",
                    "無法讀取此地圖檔案：檔案已損壞或不是地圖。",
                ])
                .to_string(),
            crate::custom_maps::Blocker::Mission(_) | crate::custom_maps::Blocker::ConvoyPath => {
                let tail = self.pick([
                    "Edit the map to play it.",
                    "Upravte mapu, aby sa dala hrať.",
                    "Upravte mapu, aby se dala hrát.",
                    "请先编辑地图。",
                    "請先編輯地圖。",
                ]);
                let stop = if self.0 >= 3 { "。" } else { ". " };
                let what = match b {
                    crate::custom_maps::Blocker::Mission(p) => self.problem(*p).to_string(),
                    _ => self.convoy_path().to_string(),
                };
                format!("{what}{stop}{tail}")
            }
            crate::custom_maps::Blocker::NoHeadquarters(side) => {
                let s = self.side(*side == 1);
                match self.0 {
                    1 => format!("{s}: chýba veliteľstvo. Upravte mapu, aby sa dala hrať."),
                    2 => format!("{s}: chybí velitelství. Upravte mapu, aby se dala hrát."),
                    3 => format!("{s}：没有指挥部。请先编辑地图。"),
                    4 => format!("{s}：沒有指揮部。請先編輯地圖。"),
                    _ => format!("{s}: no headquarters. Edit the map to play it."),
                }
            }
        }
    }

    /// The main menu row (lower case in Slovak / Czech, like the original rows).
    pub fn editor_row(self) -> &'static str {
        self.pick(["Map editor", "Editor máp", "Editor map", "地图编辑器", "地圖編輯器"])
    }

    pub fn editor_title(self) -> &'static str {
        self.pick(["MAP EDITOR", "EDITOR MÁP", "EDITOR MAP", "地图编辑器", "地圖編輯器"])
    }

    pub fn editor_subtitle(self) -> &'static str {
        self.pick([
            "Make your own skirmish maps",
            "Vytvorte si vlastné mapy pre potyčky",
            "Vytvořte si vlastní mapy pro potyčky",
            "制作你自己的遭遇战地图",
            "製作你自己的遭遇戰地圖",
        ])
    }

    pub fn your_maps(self) -> &'static str {
        self.pick(["YOUR MAPS", "VAŠE MAPY", "VAŠE MAPY", "你的地图", "你的地圖"])
    }

    pub fn no_maps_yet(self) -> &'static str {
        self.pick([
            "No maps yet: pick a terrain and create one.",
            "Zatiaľ žiadne mapy: vyberte terén a vytvorte novú.",
            "Zatím žádné mapy: vyberte terén a vytvořte novou.",
            "还没有地图：选择地形并创建一张。",
            "還沒有地圖：選擇地形並建立一張。",
        ])
    }

    pub fn new_map(self) -> &'static str {
        self.pick(["NEW MAP", "NOVÁ MAPA", "NOVÁ MAPA", "新地图", "新地圖"])
    }

    /// The default name of a new map.
    pub fn untitled(self) -> &'static str {
        self.pick(["New map", "Nová mapa", "Nová mapa", "新地图", "新地圖"])
    }

    pub fn create(self) -> &'static str {
        self.pick(["CREATE", "VYTVORIŤ", "VYTVOŘIT", "创建", "建立"])
    }

    pub fn open(self) -> &'static str {
        self.pick(["Open", "Otvoriť", "Otevřít", "打开", "開啟"])
    }

    pub fn map_name(self) -> &'static str {
        self.pick(["Map name", "Názov mapy", "Název mapy", "地图名称", "地圖名稱"])
    }

    pub fn saves_as(self, file: &str) -> String {
        match self.0 {
            1 => format!("Uloží sa ako {file}"),
            2 => format!("Uloží se jako {file}"),
            3 => format!("保存为 {file}"),
            4 => format!("儲存為 {file}"),
            _ => format!("Saves as {file}"),
        }
    }

    pub fn save(self) -> &'static str {
        self.pick(["Save", "Uložiť", "Uložit", "保存", "儲存"])
    }

    pub fn state_unsaved(self) -> &'static str {
        self.pick(["Unsaved changes", "Neuložené zmeny", "Neuložené změny", "未保存的更改", "未儲存的變更"])
    }

    pub fn state_saved(self) -> &'static str {
        self.pick(["Saved", "Uložené", "Uloženo", "已保存", "已儲存"])
    }

    pub fn state_new(self) -> &'static str {
        self.pick(["Not saved yet", "Zatiaľ neuložená", "Zatím neuložena", "尚未保存", "尚未儲存"])
    }

    pub fn saved_to_folder(self) -> &'static str {
        self.pick(["Saved to the maps folder", "Uložené do priečinka máp", "Uloženo do složky map", "已保存到地图文件夹", "已儲存到地圖資料夾"])
    }

    pub fn overwrite(self, file: &str) -> String {
        match self.0 {
            1 => format!("{file} už existuje: ďalšie Uložiť ho nahradí"),
            2 => format!("{file} už existuje: další Uložit jej nahradí"),
            3 => format!("{file} 已存在：再次保存将替换它"),
            4 => format!("{file} 已存在：再次儲存將取代它"),
            _ => format!("{file} exists: Save again to replace it"),
        }
    }

    pub fn cannot_save(self, e: &str) -> String {
        match self.0 {
            1 => format!("Nedá sa uložiť: {e}"),
            2 => format!("Nelze uložit: {e}"),
            3 => format!("无法保存：{e}"),
            4 => format!("無法儲存：{e}"),
            _ => format!("Cannot save: {e}"),
        }
    }

    pub fn enter_name(self) -> &'static str {
        self.pick(["Enter a map name", "Zadajte názov mapy", "Zadejte název mapy", "请输入地图名称", "請輸入地圖名稱"])
    }

    pub fn leave_unsaved(self) -> &'static str {
        self.pick([
            "Unsaved changes: press Back again to leave without saving",
            "Neuložené zmeny: ďalším Späť odídete bez uloženia",
            "Neuložené změny: dalším Zpět odejdete bez uložení",
            "有未保存的更改：再按一次返回将不保存而离开",
            "有未儲存的變更：再按一次返回將不儲存而離開",
        ])
    }

    pub fn place(self) -> &'static str {
        self.pick(["PLACE", "UMIESTNIŤ", "UMÍSTIT", "放置", "放置"])
    }

    /// Owner names: neutral, blue, green.
    pub fn owner(self, side: usize) -> &'static str {
        [["Neutral", "Blue", "Green"], ["Neutrálna", "Modrá", "Zelená"], ["Neutrální", "Modrá", "Zelená"], ["中立", "蓝队", "绿队"], ["中立", "藍隊", "綠隊"]]
            [self.0][side.min(2)]
    }

    /// The palette's tabs: buildings, turrets, units.
    pub fn palette_tab(self, k: usize) -> &'static str {
        [["Buildings", "Turrets", "Units"], ["Budovy", "Veže", "Jednotky"], ["Budovy", "Věže", "Jednotky"], ["建筑", "炮塔", "单位"], ["建築", "砲塔", "單位"]]
            [self.0][k.min(2)]
    }

    /// The editor's mission card and its rows.
    pub fn mission_card(self) -> &'static str {
        self.pick(["MISSION", "MISIA", "MISE", "任务", "任務"])
    }

    pub fn countdown(self) -> &'static str {
        self.pick(["Countdown", "Odpočet", "Odpočet", "倒计时", "倒計時"])
    }

    /// No countdown, hold out until it ends, win before it ends.
    pub fn countdown_mode(self, k: usize) -> &'static str {
        [
            ["None", "Hold out", "Time limit", "Reinforcements"],
            ["Žiadny", "Vydržať", "Časový limit", "Posily"],
            ["Žádný", "Vydržet", "Časový limit", "Posily"],
            ["无", "坚守", "限时", "增援"],
            ["無", "堅守", "限時", "增援"],
        ][self.0][k.min(3)]
    }

    /// What a reinforcement countdown brings.
    pub fn arrives(self) -> &'static str {
        self.pick(["Arrives at 0:00", "Príde o 0:00", "Dorazí v 0:00", "0:00 时到达", "0:00 時到達"])
    }

    /// "Landing points: 2 / 6" (where carryalls land squads).
    pub fn landings(self, n: usize, max: usize) -> String {
        let head = self.pick(["Landing points", "Miesta výsadku", "Místa výsadku", "空投点", "空投點"]);
        let colon = if self.0 >= 3 { "：" } else { ": " };
        format!("{head}{colon}{n} / {max}")
    }

    pub fn add_on_map(self) -> &'static str {
        self.pick(["Add on map", "Pridať na mape", "Přidat na mapě", "在地图上添加", "在地圖上添加"])
    }

    /// "Squad 4" (carryall squads, numbered from 1).
    pub fn squad(self, n: usize) -> String {
        match self.0 {
            1 | 2 => format!("Čata {n}"),
            3 => format!("小队 {n}"),
            4 => format!("小隊 {n}"),
            _ => format!("Squad {n}"),
        }
    }

    /// A landing point the map editor refuses.
    pub fn cannot_land(self, why: crate::mapeditor_ui::LandingWhy) -> &'static str {
        use crate::mapeditor_ui::LandingWhy;
        match why {
            LandingWhy::Ground => self.pick([
                "No room for a whole squad: blocked ground or a building",
                "Nie je tu miesto pre celú čatu: neprechodný terén alebo budova",
                "Není tu místo pro celou četu: neprůchodný terén nebo budova",
                "此处无法容纳整个小队：地形受阻或有建筑",
                "此處無法容納整個小隊：地形受阻或有建築",
            ]),
            LandingWhy::Overlap => self.pick([
                "Too close to another landing point",
                "Príliš blízko iného miesta výsadku",
                "Příliš blízko jiného místa výsadku",
                "离另一个空投点太近",
                "離另一個空投點太近",
            ]),
        }
    }

    /// The checks card's line for landing point `n` (numbered from 1).
    pub fn landing_problem(self, n: usize, why: crate::mapeditor_ui::LandingWhy) -> String {
        use crate::mapeditor_ui::LandingWhy;
        let what = match why {
            LandingWhy::Ground => self.pick([
                "blocked ground or a building",
                "neprechodný terén alebo budova",
                "neprůchodný terén nebo budova",
                "地形受阻或有建筑",
                "地形受阻或有建築",
            ]),
            LandingWhy::Overlap => self.pick(["overlaps another", "prekrýva sa s iným", "překrývá se s jiným", "与其他空投点重叠", "與其他空投點重疊"]),
        };
        match self.0 {
            1 => format!("Miesto výsadku {n}: {what}"),
            2 => format!("Místo výsadku {n}: {what}"),
            3 => format!("空投点 {n}：{what}"),
            4 => format!("空投點 {n}：{what}"),
            _ => format!("Landing point {n}: {what}"),
        }
    }

    pub fn click_landing(self) -> &'static str {
        self.pick([
            "Click the map to set a landing point",
            "Kliknutím na mapu určíš miesto výsadku",
            "Kliknutím na mapu určíš místo výsadku",
            "点击地图设置空投点",
            "點擊地圖設定空投點",
        ])
    }

    pub fn minutes(self) -> &'static str {
        self.pick(["Minutes", "Minúty", "Minuty", "分钟", "分鐘"])
    }

    pub fn to_win(self) -> &'static str {
        self.pick(["To win", "Víťazstvo", "Vítězství", "胜利条件", "勝利條件"])
    }

    /// The enemy's headquarters, or everything of the enemy.
    pub fn win_rule(self, k: usize) -> &'static str {
        [
            ["Enemy HQ", "Everything", "Escort"],
            ["Veliteľstvá", "Všetko", "Eskorta"],
            ["Velitelství", "Všechno", "Eskorta"],
            ["摧毁指挥部", "全歼", "护送"],
            ["摧毀指揮部", "全殲", "護送"],
        ][self.0][k.min(2)]
    }

    pub fn convoy(self) -> &'static str {
        self.pick(["Convoy", "Konvoj", "Konvoj", "护送队", "護送隊"])
    }

    pub fn no_pods_hint(self) -> &'static str {
        self.pick([
            "none: place blue pods (Units tab)",
            "žiadny: umiestni modré kapsuly (Jednotky)",
            "žádný: umísti modré kapsle (Jednotky)",
            "无：放置蓝队单位（单位页）",
            "無：放置藍隊單位（單位頁）",
        ])
    }

    pub fn goal(self) -> &'static str {
        self.pick(["Goal", "Cieľ", "Cíl", "目标", "目標"])
    }

    pub fn remove_last(self) -> &'static str {
        self.pick(["Remove last", "Odobrať posledný", "Odebrat poslední", "移除最后一个", "移除最後一個"])
    }

    pub fn set_on_map(self) -> &'static str {
        self.pick(["Set on map", "Určiť na mape", "Určit na mapě", "在地图上设置", "在地圖上設定"])
    }

    /// "Route: 2 / 8" (waypoints).
    pub fn route(self, n: usize, max: usize) -> String {
        let head = self.pick(["Route", "Trasa", "Trasa", "路线", "路線"]);
        let colon = if self.0 >= 3 { "：" } else { ": " };
        format!("{head}{colon}{n} / {max}")
    }

    pub fn click_goal(self) -> &'static str {
        self.pick(["Click the map to set the goal", "Kliknutím na mapu urči cieľ", "Kliknutím na mapu urči cíl", "点击地图设置目标", "點擊地圖設定目標"])
    }

    pub fn click_waypoint(self) -> &'static str {
        self.pick([
            "Click the map to add waypoints in order",
            "Klikaním na mapu pridávaj body trasy",
            "Klikáním na mapu přidávej body trasy",
            "依次点击地图添加路点",
            "依次點擊地圖添加路點",
        ])
    }

    /// The checks card's line (and the skirmish screen's reason) when the convoy cannot walk every leg of its route.
    pub fn convoy_path(self) -> &'static str {
        self.pick(["Invalid convoy path", "Neplatná trasa konvoja", "Neplatná trasa konvoje", "护送队路线无效", "護送隊路線無效"])
    }

    /// The skirmish screen's line about a custom map's mission (`None`: a plain skirmish).
    pub fn objective(self, timer: Option<opensow_formats::custommap::Timer>, win: opensow_formats::custommap::WinRule) -> Option<String> {
        use opensow_formats::custommap::TimerEnd;
        use opensow_formats::custommap::WinRule;
        let clock = timer.map(|t| {
            let n = t.minutes;
            match (t.end, self.0) {
                (TimerEnd::Victory, 1 | 2) => format!("Vydrž {n} min"),
                (TimerEnd::Victory, 3) => format!("坚守 {n} 分钟"),
                (TimerEnd::Victory, 4) => format!("堅守 {n} 分鐘"),
                (TimerEnd::Victory, _) => format!("Hold out for {n} min"),
                (TimerEnd::Defeat, 1 | 2) => format!("Vyhraj do {n} min"),
                (TimerEnd::Defeat, 3) => format!("{n} 分钟内获胜"),
                (TimerEnd::Defeat, 4) => format!("{n} 分鐘內獲勝"),
                (TimerEnd::Defeat, _) => format!("Win within {n} min"),
                (TimerEnd::Reinforcements, 1) => format!("Posily o {n} min"),
                (TimerEnd::Reinforcements, 2) => format!("Posily za {n} min"),
                (TimerEnd::Reinforcements, 3) => format!("{n} 分钟后增援"),
                (TimerEnd::Reinforcements, 4) => format!("{n} 分鐘後增援"),
                (TimerEnd::Reinforcements, _) => format!("Reinforcements in {n} min"),
            }
        });
        let all = match win {
            WinRule::Headquarters => None,
            WinRule::Everything => Some(self.pick(["Destroy everything", "Znič všetko", "Znič všechno", "消灭全部敌军", "消滅全部敵軍"]).to_string()),
            WinRule::Escort => Some(self.pick(["Escort the convoy", "Doprav konvoj", "Doprav konvoj", "护送车队", "護送車隊"]).to_string()),
        };
        let parts: Vec<String> = clock.into_iter().chain(all).collect();
        (!parts.is_empty()).then(|| parts.join("  ·  "))
    }

    pub fn sides(self) -> &'static str {
        self.pick(["SIDES", "STRANY", "STRANY", "双方", "雙方"])
    }

    pub fn ready(self) -> &'static str {
        self.pick(["Ready to play", "Pripravená na hru", "Připravena ke hře", "可以开始游戏", "可以開始遊戲"])
    }

    pub fn problem(self, p: opensow_formats::custommap::Problem) -> String {
        use opensow_formats::custommap::Problem;
        match p {
            Problem::NoHeadquarters(side) => {
                let s = self.owner(side + 1);
                match self.0 {
                    1 => format!("{s}: chýba veliteľstvo"),
                    2 => format!("{s}: chybí velitelství"),
                    3 => format!("{s}：没有指挥部"),
                    4 => format!("{s}：沒有指揮部"),
                    _ => format!("{s}: no headquarters"),
                }
            }
            Problem::NoAdvancer(side) => {
                let s = self.owner(side + 1);
                match self.0 {
                    1 => format!("{s}: bez Advancera, nemôže dobývať"),
                    2 => format!("{s}: bez PVT, nemůže dobývat"),
                    3 => format!("{s}：没有 Advancer，无法占领"),
                    4 => format!("{s}：沒有 Advancer，無法佔領"),
                    _ => format!("{s}: no Advancer, cannot capture"),
                }
            }
            Problem::NoPods => self
                .pick([
                    "Escort: blue has no pods for the convoy",
                    "Eskorta: modrá nemá kapsuly do konvoja",
                    "Eskorta: modrá nemá kapsle do konvoje",
                    "护送：蓝队没有护送单位",
                    "護送：藍隊沒有護送單位",
                ])
                .to_string(),
            Problem::NoGoal => {
                self.pick(["Escort: no goal set", "Eskorta: chýba cieľ", "Eskorta: chybí cíl", "护送：未设置目标", "護送：未設定目標"]).to_string()
            }
        }
    }

    pub fn faction(self) -> &'static str {
        self.pick(["FACTION", "STRANA", "STRANA", "阵营", "陣營"])
    }

    pub fn start_health(self) -> &'static str {
        self.pick(["START HEALTH", "POČIATOČNÉ ZDRAVIE", "POČÁTEČNÍ ZDRAVÍ", "初始生命值", "初始生命值"])
    }

    pub fn default_value(self) -> &'static str {
        self.pick(["Default", "Predvolené", "Výchozí", "默认", "預設"])
    }

    pub fn cell(self, x: i32, y: i32) -> String {
        match self.0 {
            1 | 2 => format!("Políčko {x},{y}"),
            3 | 4 => format!("格子 {x},{y}"),
            _ => format!("Cell {x},{y}"),
        }
    }

    pub fn production(self) -> &'static str {
        self.pick(["PRODUCTION", "VÝROBA", "VÝROBA", "生产", "生產"])
    }

    pub fn levels(self) -> &'static str {
        self.pick(["LEVELS", "ÚROVNE", "ÚROVNĚ", "等级", "等級"])
    }

    pub fn unlocked(self) -> &'static str {
        self.pick(["unlocked", "odomknutá", "odemčená", "已解锁", "已解鎖"])
    }

    pub fn add_line(self) -> &'static str {
        self.pick(["Add line", "Pridať linku", "Přidat linku", "添加生产线", "新增生產線"])
    }

    pub fn add_level(self) -> &'static str {
        self.pick(["Add level", "Pridať úroveň", "Přidat úroveň", "添加等级", "新增等級"])
    }

    /// The hint under the production lines.
    pub fn lines_hint(self) -> &'static str {
        self.pick([
            "The lock: unlocked at the start, or unlocked by the Advancer with the points shown (in order).",
            "Zámok: odomknutá od začiatku, alebo ju odomkne Advancer zobrazenými bodmi (postupne).",
            "Zámek: odemčená od začátku, nebo ji odemkne PVT zobrazenými body (postupně).",
            "锁：开局即解锁，或由 Advancer 用显示的点数依次解锁。",
            "鎖：開局即解鎖，或由 Advancer 用顯示的點數依次解鎖。",
        ])
    }

    pub fn turret_type(self) -> &'static str {
        self.pick(["TYPE", "TYP", "TYP", "类型", "類型"])
    }

    pub fn on_off(self, on: bool) -> &'static str {
        if on {
            self.pick(["On", "Áno", "Ano", "开", "開"])
        } else {
            self.pick(["Off", "Nie", "Ne", "关", "關"])
        }
    }

    pub fn units_never_neutral(self) -> &'static str {
        self.pick([
            "Units belong to blue or green",
            "Jednotky patria modrej alebo zelenej",
            "Jednotky patří modré nebo zelené",
            "单位只能属于蓝队或绿队",
            "單位只能屬於藍隊或綠隊",
        ])
    }

    pub fn cannot_place(self) -> &'static str {
        self.pick(["It does not fit there", "Sem sa to nezmestí", "Sem se to nevejde", "无法放在这里", "無法放在這裡"])
    }

    pub fn build_areas(self) -> &'static str {
        self.pick([
            "Show where nothing can be built (B)",
            "Ukázať, kde sa nedá stavať (B)",
            "Ukázat, kde nelze stavět (B)",
            "显示不可建造的区域 (B)",
            "顯示不可建造的區域 (B)",
        ])
    }

    pub fn delete(self) -> &'static str {
        self.pick(["Delete", "Odstrániť", "Odstranit", "删除", "刪除"])
    }

    pub fn delete_sure(self) -> &'static str {
        self.pick(["Sure? Delete", "Naozaj? Odstrániť", "Opravdu? Odstranit", "确定删除？", "確定刪除？"])
    }

    pub fn done(self) -> &'static str {
        self.pick(["Done", "Hotovo", "Hotovo", "完成", "完成"])
    }

    pub fn last_headquarters(self) -> &'static str {
        self.pick([
            "A side keeps its last headquarters",
            "Strana si ponechá posledné veliteľstvo",
            "Strana si ponechá poslední velitelství",
            "每方至少保留一个指挥部",
            "每方至少保留一個指揮部",
        ])
    }

    // ---- Options page 2 (this port's settings, docs/options.md) ----

    /// The heading over page 2's rows (in the register of the page-1 headings: GFX / Grafika).
    pub fn enhancements(self) -> &'static str {
        self.pick(["ENHANCEMENTS", "Vylepšenia", "Vylepšení", "增强", "增強"])
    }

    /// The resolution row's caption when the game draws at the window's own size.
    pub fn native(self) -> &'static str {
        self.pick(["NATIVE", "Natívne", "Nativní", "原生", "原生"])
    }

    /// Page 2's rows in [`crate::options::OWN_ROWS`] order: (label, one-line explanation).
    pub fn option_rows(self) -> [(&'static str, &'static str); 9] {
        match self.0 {
            1 => [
                ("Vylepšené textúry", "Vyhladí jednotky, budovy a terén pri priblížení"),
                ("Mäkké tiene", "Rozmaže okraje tieňov"),
                ("Kontaktné tiene", "Tmavý lem, kde sa budovy a jednotky dotýkajú zeme"),
                ("Žiara výbuchov", "Výbuchy a záblesky výstrelov osvetľujú terén"),
                ("Tiene veží", "Každá veža vrhá tieň (pôvodne len protilietadlová)"),
                ("Vylepšený zvuk", "Čistejšie a plynulejšie efekty, reč a hudba"),
                ("Dosah veží", "Pri umiestňovaní veže ukáže, kam dostrelí"),
                ("Priblíženie mapy", "Približovanie kolieskom myši (pôvodne pevná mierka)"),
                ("Prehodiť tlačidlá myši", "Rozkazy na presun a útok pravým tlačidlom"),
            ],
            2 => [
                ("Vylepšené textury", "Vyhladí jednotky, budovy a terén při přiblížení"),
                ("Měkké stíny", "Rozmaže okraje stínů"),
                ("Kontaktní stíny", "Tmavý lem, kde se budovy a jednotky dotýkají země"),
                ("Záře výbuchů", "Výbuchy a záblesky výstřelů osvětlují terén"),
                ("Stíny věží", "Každá věž vrhá stín (původně jen protiletadlová)"),
                ("Vylepšený zvuk", "Čistší a plynulejší efekty, řeč a hudba"),
                ("Dosah věží", "Při umísťování věže ukáže, kam dostřelí"),
                ("Přiblížení mapy", "Přibližování kolečkem myši (původně pevné měřítko)"),
                ("Prohodit tlačítka myši", "Rozkazy k přesunu a útoku pravým tlačítkem"),
            ],
            3 => [
                ("增强纹理", "放大时平滑单位、建筑和地面"),
                ("柔和阴影", "模糊阴影边缘"),
                ("接触阴影", "建筑和单位与地面接触处的暗晕"),
                ("爆炸光晕", "爆炸和枪口火光照亮地面"),
                ("炮塔阴影", "每座炮塔都投下阴影（原版仅防空炮塔）"),
                ("增强音效", "更清晰、更平滑的音效、语音和音乐"),
                ("炮塔射程", "放置炮塔时显示其射程"),
                ("地图缩放", "用鼠标滚轮缩放（原版为固定比例）"),
                ("交换鼠标按键", "用右键下达移动和攻击命令"),
            ],
            4 => [
                ("增強紋理", "放大時平滑單位、建築和地面"),
                ("柔和陰影", "模糊陰影邊緣"),
                ("接觸陰影", "建築和單位與地面接觸處的暗暈"),
                ("爆炸光暈", "爆炸和槍口火光照亮地面"),
                ("砲塔陰影", "每座砲塔都投下陰影（原版僅防空砲塔）"),
                ("增強音效", "更清晰、更平滑的音效、語音和音樂"),
                ("砲塔射程", "放置砲塔時顯示其射程"),
                ("地圖縮放", "用滑鼠滾輪縮放（原版為固定比例）"),
                ("交換滑鼠按鍵", "用右鍵下達移動和攻擊命令"),
            ],
            _ => crate::options::OWN_ROWS,
        }
    }

    /// Heading of Controls page `k` (0 camera, 1 units and orders, 2 air force and game; `keybinds::PAGE_ROWS`).
    pub fn controls_heading(self, k: usize) -> &'static str {
        let t = match k {
            0 => ["CAMERA", "KAMERA", "KAMERA", "镜头", "鏡頭"],
            1 => ["UNITS", "JEDNOTKY", "JEDNOTKY", "单位", "單位"],
            _ => ["GAME", "HRA", "HRA", "游戏", "遊戲"],
        };
        self.pick(t)
    }

    /// The button on the options pages that opens the key bindings.
    pub fn controls_button(self) -> &'static str {
        self.pick(["CONTROLS", "OVLÁDANIE", "OVLÁDÁNÍ", "控制", "控制"])
    }

    /// The title of the Controls view.
    pub fn controls_title(self) -> &'static str {
        self.pick(["Controls", "Ovládanie", "Ovládání", "控制", "控制"])
    }

    /// The button that puts every key back to its default.
    pub fn defaults(self) -> &'static str {
        self.pick(["DEFAULTS", "PÔVODNÉ", "VÝCHOZÍ", "默认", "預設"])
    }

    /// The help line of the Controls pages.
    pub fn controls_hint(self) -> &'static str {
        self.pick([
            "Click a slot, then press a key or mouse button. Right click clears it.",
            "Klikni na políčko a stlač kláves alebo tlačidlo myši. Pravé tlačidlo ho zmaže.",
            "Klikni na políčko a stiskni klávesu nebo tlačítko myši. Pravé tlačítko ho smaže.",
            "点击按键格后按下键盘键或鼠标键。右键清除。",
            "點擊按鍵格後按下鍵盤鍵或滑鼠鍵。右鍵清除。",
        ])
    }

    /// Shown in the slot that waits for a key.
    pub fn press_key(self) -> &'static str {
        self.pick(["press a key", "stlač kláves", "stiskni klávesu", "请按键", "請按鍵"])
    }

    /// The help line while a slot waits for a key.
    pub fn capture_hint(self) -> &'static str {
        self.pick([
            "Press a key or mouse button (Esc cancels).",
            "Stlač kláves alebo tlačidlo myši (Esc zruší).",
            "Stiskni klávesu nebo tlačítko myši (Esc zruší).",
            "请按键盘键或鼠标键（Esc 取消）。",
            "請按鍵盤鍵或滑鼠鍵（Esc 取消）。",
        ])
    }

    /// The help line after a key that cannot be bound.
    pub fn key_fixed(self) -> &'static str {
        self.pick([
            "That key cannot be used: Esc, Shift, Ctrl, Alt and the left / right mouse buttons are fixed.",
            "Tento kláves sa nedá použiť: Esc, Shift, Ctrl, Alt a ľavé / pravé tlačidlo myši sú pevné.",
            "Tuto klávesu nelze použít: Esc, Shift, Ctrl, Alt a levé / pravé tlačítko myši jsou pevné.",
            "该键不可使用：Esc、Shift、Ctrl、Alt 和鼠标左右键是固定的。",
            "該鍵不可使用：Esc、Shift、Ctrl、Alt 和滑鼠左右鍵是固定的。",
        ])
    }

    /// The help line after a key was taken from another action.
    pub fn key_moved(self, key: &str, from: &str) -> String {
        match self.0 {
            1 => format!("{key} už nerobí: {from}"),
            2 => format!("{key} už nedělá: {from}"),
            3 => format!("{key} 已从“{from}”移除"),
            4 => format!("{key} 已從「{from}」移除"),
            _ => format!("{key} no longer does: {from}"),
        }
    }

    /// The row label of a key action on the Controls pages.
    pub fn bind_label(self, act: crate::keybinds::Act) -> String {
        use crate::keybinds::Act as A;
        let l = self.0;
        let t = |t: [&'static str; 5]| t[l].to_owned();
        match act {
            A::ScrollUp => t(["Scroll up", "Posun hore", "Posun nahoru", "向上滚动", "向上捲動"]),
            A::ScrollDown => t(["Scroll down", "Posun dole", "Posun dolů", "向下滚动", "向下捲動"]),
            A::ScrollLeft => t(["Scroll left", "Posun vľavo", "Posun vlevo", "向左滚动", "向左捲動"]),
            A::ScrollRight => t(["Scroll right", "Posun vpravo", "Posun vpravo", "向右滚动", "向右捲動"]),
            A::LookAtHq => t(["View headquarters", "Pohľad na veliteľstvo", "Pohled na velitelství", "查看总部", "查看總部"]),
            A::LookAtEvent => t(["View last message", "Pohľad na poslednú správu", "Pohled na poslední zprávu", "查看最近消息", "查看最近訊息"]),
            A::CentreSelection => t(["Centre on selection", "Vycentrovať na výber", "Vycentrovat na výběr", "居中到所选单位", "置中到所選單位"]),
            A::Bookmark(k) => {
                let n = k + 1;
                match l {
                    1 | 2 => format!("Záložka kamery {n} (Shift: ulož)"),
                    3 => format!("镜头书签 {n}（Shift：设置）"),
                    4 => format!("鏡頭書籤 {n}（Shift：設定）"),
                    _ => format!("Camera bookmark {n} (Shift: set)"),
                }
            }
            A::NextUnit => t(["Next unit", "Ďalšia jednotka", "Další jednotka", "下一个单位", "下一個單位"]),
            A::NextAdvancer => t([
                "Next Advancer (Shift: idle)",
                "Ďalší Advancer (Shift: nečinný)",
                "Další Advancer (Shift: nečinný)",
                "下一个工程车（Shift：空闲）",
                "下一個工程車（Shift：閒置）",
            ]),
            A::SelectVisible => {
                t(["Select units on screen", "Vybrať jednotky na obrazovke", "Vybrat jednotky na obrazovce", "选择屏幕上的单位", "選擇畫面上的單位"])
            }
            A::SelectVisibleAntiAir => t([
                "Select anti-air on screen",
                "Vybrať protivzdušné na obrazovke",
                "Vybrat protivzdušné na obrazovce",
                "选择屏幕上的防空单位",
                "選擇畫面上的防空單位",
            ]),
            A::SelectVisibleGround => {
                t(["Select others on screen", "Vybrať ostatné na obrazovke", "Vybrat ostatní na obrazovce", "选择屏幕上的其他单位", "選擇畫面上的其他單位"])
            }
            A::SelectAll => t(["Select all units", "Vybrať všetky jednotky", "Vybrat všechny jednotky", "选择全部单位", "選擇全部單位"]),
            A::Scatter => t(["Scatter", "Rozptýliť", "Rozprchnout", "散开", "散開"]),
            A::RallyOrRepair => t(["Rally point / repair", "Zhromaždisko / oprava", "Shromaždiště / oprava", "集结点 / 修理", "集結點 / 修理"]),
            A::Turret(k) => {
                let n = k + 1;
                match l {
                    1 => format!("Postaviť vežu {n}"),
                    2 => format!("Postavit věž {n}"),
                    3 => format!("建造炮塔 {n}"),
                    4 => format!("建造砲塔 {n}"),
                    _ => format!("Build turret {n}"),
                }
            }
            A::StrikeBomber => t(["Air strike: Bomber", "Letecký útok: bombardér", "Letecký útok: bombardér", "空袭：轰炸机", "空襲：轟炸機"]),
            A::StrikeAsteroid => t(["Air strike: Meteor storm", "Letecký útok: meteory", "Letecký útok: meteory", "空袭：流星雨", "空襲：流星雨"]),
            A::StrikeCarryall => t(["Air strike: CarryAll", "Letecký útok: CarryAll", "Letecký útok: CarryAll", "空袭：运输机", "空襲：運輸機"]),
            A::StrikeTripler => t(["Air strike: Tripler", "Letecký útok: Tripler", "Letecký útok: Tripler", "空袭：三联机", "空襲：三聯機"]),
            A::StrikeFighter => t(["Air strike: Fighter", "Letecký útok: stíhačka", "Letecký útok: stíhačka", "空袭：战斗机", "空襲：戰鬥機"]),
            A::TechUp => t([
                "Buy all affordable upgrades",
                "Kúpiť všetky dostupné vylepšenia",
                "Koupit všechna dostupná vylepšení",
                "购买所有可负担的升级",
                "購買所有可負擔的升級",
            ]),
            A::Pause => t(["Pause", "Pauza", "Pauza", "暂停", "暫停"]),
            A::Chat => t(["Chat", "Chat", "Chat", "聊天", "聊天"]),
            A::KeyHelp => t(["Key help", "Pomoc ku klávesom", "Nápověda ke klávesám", "按键帮助", "按鍵說明"]),
            A::Abandon => t(["Leave mission", "Opustiť misiu", "Opustit misi", "放弃任务", "放棄任務"]),
            A::SpeedDown => t(["Slower game", "Pomalšia hra", "Pomalejší hra", "减慢游戏", "減慢遊戲"]),
            A::SpeedUp => t(["Faster game", "Rýchlejšia hra", "Rychlejší hra", "加快游戏", "加快遊戲"]),
            A::QuickSave => t([
                "Quick save (Shift: load)",
                "Rýchle uloženie (Shift: načítať)",
                "Rychlé uložení (Shift: načíst)",
                "快速存档（Shift：读取）",
                "快速存檔（Shift：讀取）",
            ]),
            A::Fullscreen => t(["Full screen", "Celá obrazovka", "Celá obrazovka", "全屏", "全螢幕"]),
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
            for s in [
                t.menu_row(),
                t.title(),
                t.star_none(),
                t.setup_default(),
                t.setup_changed(),
                t.subtitle(),
                t.swap(),
                t.reset(),
                t.start(),
                t.back(),
                t.tab_original(),
                t.tab_custom(),
                t.open_folder(),
                t.edit_map(),
                t.no_custom_maps(),
                t.editor_row(),
                t.editor_title(),
                t.your_maps(),
                t.new_map(),
                t.create(),
                t.map_name(),
                t.save(),
                t.place(),
                t.sides(),
                t.ready(),
                t.faction(),
                t.start_health(),
                t.production(),
                t.levels(),
                t.add_line(),
                t.lines_hint(),
                t.turret_type(),
            ] {
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
