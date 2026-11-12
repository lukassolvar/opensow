//! The launcher's texts in the languages the port offers (`Language::SUPPORTED`: English, Slovak, Czech, Chinese simplified and
//! traditional). The page switches as soon as a language is clicked; "From game files" reads the install's `Data\lang.dat`.

use crate::settings::Field;
use crate::update::Fail;
use opensow_formats::lang::Language;

/// Index into the five-language tables: English, Slovak, Czech, Chinese (simplified), Chinese (traditional).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tx(usize);

impl Tx {
    pub fn new(l: Language) -> Tx {
        Tx(match l {
            Language::Slovak => 1,
            Language::Czech => 2,
            Language::ChineseSimplified => 3,
            Language::ChineseTraditional => 4,
            _ => 0,
        })
    }

    pub fn english() -> Tx {
        Tx(0)
    }

    fn pick(self, t: [&'static str; 5]) -> &'static str {
        t[self.0]
    }

    pub fn window_title(self) -> &'static str {
        self.pick(["OpenSOW Launcher", "OpenSOW – spúšťač", "OpenSOW – spouštěč", "OpenSOW 启动器", "OpenSOW 啟動器"])
    }

    pub fn launcher(self) -> &'static str {
        self.pick(["Launcher", "Spúšťač", "Spouštěč", "启动器", "啟動器"])
    }

    pub fn home(self) -> &'static str {
        self.pick(["Home", "Domov", "Domů", "主页", "首頁"])
    }

    pub fn settings(self) -> &'static str {
        self.pick(["Settings", "Nastavenia", "Nastavení", "设置", "設定"])
    }

    pub fn language(self) -> &'static str {
        self.pick(["Language", "Jazyk", "Jazyk", "语言", "語言"])
    }

    pub fn game_files_label(self) -> &'static str {
        self.pick(["From game files", "Z herných súborov", "Z herních souborů", "来自游戏文件", "來自遊戲檔案"])
    }

    pub fn auto_detect(self) -> &'static str {
        self.pick(["Auto-detect", "Nájsť automaticky", "Najít automaticky", "自动检测", "自動偵測"])
    }

    pub fn play(self) -> &'static str {
        self.pick(["Play", "Hrať", "Hrát", "开始游戏", "開始遊戲"])
    }

    /// A game's state in the sidebar and on the home page.
    pub fn ready(self) -> &'static str {
        self.pick(["Ready to play", "Pripravené na hru", "Připraveno ke hře", "可以开始游戏", "可以開始遊戲"])
    }

    pub fn not_located(self) -> &'static str {
        self.pick(["Not located", "Nenájdené", "Nenalezeno", "未找到", "未找到"])
    }

    pub fn files_missing(self) -> &'static str {
        self.pick(["Game files missing", "Chýbajú herné súbory", "Chybí herní soubory", "缺少游戏文件", "缺少遊戲檔案"])
    }

    pub fn optional(self) -> &'static str {
        self.pick(["optional", "voliteľné", "volitelné", "可选", "可選"])
    }

    pub fn tagline(self) -> &'static str {
        self.pick([
            "State of War, rebuilt for today",
            "State of War v novom prevedení",
            "State of War v novém provedení",
            "重制版 State of War",
            "重製版 State of War",
        ])
    }

    pub fn your_games(self) -> &'static str {
        self.pick(["Your games", "Vaše hry", "Vaše hry", "你的游戏", "你的遊戲"])
    }

    pub fn locate_first(self) -> &'static str {
        self.pick([
            "Locate State of War to play",
            "Na hranie najprv nájdite State of War",
            "Pro hraní nejdřív najděte State of War",
            "请先找到 State of War 再开始游戏",
            "請先找到 State of War 再開始遊戲",
        ])
    }

    pub fn find_games(self) -> &'static str {
        self.pick(["Find games automatically", "Nájsť hry automaticky", "Najít hry automaticky", "自动查找游戏", "自動尋找遊戲"])
    }

    /// The line under a game's title.
    pub fn subtitle(self, warmonger: bool) -> &'static str {
        if warmonger {
            self.pick(["The second campaign · optional", "Druhá kampaň · voliteľná", "Druhá kampaň · volitelná", "第二个战役 · 可选", "第二個戰役 · 可選"])
        } else {
            self.pick(["The original campaign · 2001", "Pôvodná kampaň · 2001", "Původní kampaň · 2001", "原版战役 · 2001", "原版戰役 · 2001"])
        }
    }

    pub fn locate(self) -> &'static str {
        self.pick(["Locate game…", "Nájsť hru…", "Najít hru…", "定位游戏…", "定位遊戲…"])
    }

    pub fn change_folder(self) -> &'static str {
        self.pick(["Change folder…", "Zmeniť priečinok…", "Změnit složku…", "更改文件夹…", "更改資料夾…"])
    }

    pub fn files_found(self) -> &'static str {
        self.pick(["Game files found", "Herné súbory nájdené", "Herní soubory nalezeny", "已找到游戏文件", "已找到遊戲檔案"])
    }

    pub fn files_missing_long(self) -> &'static str {
        self.pick([
            "The game files are gone from this folder. Locate the game again.",
            "V tomto priečinku už herné súbory nie sú. Nájdite hru znova.",
            "V této složce už herní soubory nejsou. Najděte hru znovu.",
            "此文件夹中已没有游戏文件。请重新定位游戏。",
            "此資料夾中已沒有遊戲檔案。請重新定位遊戲。",
        ])
    }

    pub fn not_located_long(self) -> &'static str {
        self.pick([
            "Show the launcher where the game is installed, or drop its folder here.",
            "Ukážte spúšťaču, kde je hra nainštalovaná, alebo sem pretiahnite jej priečinok.",
            "Ukažte spouštěči, kde je hra nainstalovaná, nebo sem přetáhněte její složku.",
            "告诉启动器游戏安装在哪里，或将其文件夹拖到这里。",
            "告訴啟動器遊戲安裝在哪裡，或將其資料夾拖到這裡。",
        ])
    }

    /// What the folder must hold.
    pub fn folder_hint(self, warmonger: bool) -> &'static str {
        if warmonger {
            self.pick([
                "The folder with State of War Warmonger.exe",
                "Priečinok so State of War Warmonger.exe",
                "Složka se State of War Warmonger.exe",
                "包含 State of War Warmonger.exe 的文件夹",
                "包含 State of War Warmonger.exe 的資料夾",
            ])
        } else {
            self.pick([
                "The folder with State of War.exe and Data",
                "Priečinok so State of War.exe a Data",
                "Složka se State of War.exe a Data",
                "包含 State of War.exe 和 Data 的文件夹",
                "包含 State of War.exe 和 Data 的資料夾",
            ])
        }
    }

    pub fn needs_base(self) -> &'static str {
        self.pick([
            "Locate State of War too: Warmonger uses its graphics and sound.",
            "Nájdite aj State of War: Warmonger používa jeho grafiku a zvuk.",
            "Najděte i State of War: Warmonger používá jeho grafiku a zvuk.",
            "还需要 State of War：Warmonger 使用它的图形和声音。",
            "還需要 State of War：Warmonger 使用它的圖形和聲音。",
        ])
    }

    pub fn campaign_note(self) -> &'static str {
        self.pick([
            "Switch between the two campaigns on the world map in the game.",
            "Medzi dvoma kampaňami prepínate na mape sveta v hre.",
            "Mezi oběma kampaněmi přepínáte na mapě světa ve hře.",
            "在游戏的世界地图上切换两个战役。",
            "在遊戲的世界地圖上切換兩個戰役。",
        ])
    }

    pub fn tab_game(self) -> &'static str {
        self.pick(["Game", "Hra", "Hra", "游戏", "遊戲"])
    }

    pub fn tab_extras(self) -> &'static str {
        self.pick(["Enhancements", "Vylepšenia", "Vylepšení", "增强", "增強"])
    }

    pub fn graphics(self) -> &'static str {
        self.pick(["Graphics & controls", "Grafika a ovládanie", "Grafika a ovládání", "图形与控制", "圖形與控制"])
    }

    pub fn sound(self) -> &'static str {
        self.pick(["Sound", "Zvuk", "Zvuk", "声音", "聲音"])
    }

    pub fn stored_in(self) -> &'static str {
        self.pick(["Game settings are stored in", "Nastavenia hry sa ukladajú do", "Nastavení hry se ukládá do", "游戏设置保存在", "遊戲設定儲存在"])
    }

    pub fn reset(self) -> &'static str {
        self.pick(["Reset game settings", "Obnoviť nastavenia hry", "Obnovit nastavení hry", "重置游戏设置", "重設遊戲設定"])
    }

    pub fn extras_note(self) -> &'static str {
        self.pick([
            "Not in the original game: switch them off for its exact look and sound.",
            "V pôvodnej hre nie sú: vypnite ich pre jej presný vzhľad a zvuk.",
            "V původní hře nejsou: vypněte je pro její přesný vzhled a zvuk.",
            "原版游戏中没有：关闭它们即可得到原版的画面和声音。",
            "原版遊戲中沒有：關閉它們即可得到原版的畫面和聲音。",
        ])
    }

    pub fn updates(self) -> &'static str {
        self.pick(["Updates", "Aktualizácie", "Aktualizace", "更新", "更新"])
    }

    pub fn check_at_start(self) -> &'static str {
        self.pick(["Check when the launcher starts", "Kontrolovať pri spustení", "Kontrolovat při spuštění", "启动时检查", "啟動時檢查"])
    }

    pub fn check_now(self) -> &'static str {
        self.pick(["Check for updates", "Skontrolovať aktualizácie", "Zkontrolovat aktualizace", "检查更新", "檢查更新"])
    }

    pub fn release_notes(self) -> &'static str {
        self.pick(["Release notes", "Poznámky k vydaniu", "Poznámky k vydání", "发行说明", "發行說明"])
    }

    pub fn restart(self) -> &'static str {
        self.pick(["Restart launcher", "Reštartovať spúšťač", "Restartovat spouštěč", "重启启动器", "重新啟動啟動器"])
    }

    /// "Version 0.1.0", the launcher's own.
    pub fn version(self, v: &str) -> String {
        match self.0 {
            1 => format!("Verzia {v}"),
            2 => format!("Verze {v}"),
            3 => format!("版本 {v}"),
            4 => format!("版本 {v}"),
            _ => format!("Version {v}"),
        }
    }

    pub fn checking(self) -> &'static str {
        self.pick(["Looking for a new release…", "Hľadám nové vydanie…", "Hledám nové vydání…", "正在查找新版本…", "正在尋找新版本…"])
    }

    pub fn up_to_date(self) -> &'static str {
        self.pick(["This is the latest version.", "Toto je najnovšia verzia.", "Toto je nejnovější verze.", "已是最新版本。", "已是最新版本。"])
    }

    pub fn available(self, v: &str, name: &str) -> String {
        match self.0 {
            1 => format!("K dispozícii je verzia {v}: {name}"),
            2 => format!("Je k dispozici verze {v}: {name}"),
            3 => format!("有新版本 {v}：{name}"),
            4 => format!("有新版本 {v}：{name}"),
            _ => format!("Version {v} is available: {name}"),
        }
    }

    /// The sidebar's line under Settings while a new version waits.
    pub fn update_badge(self, v: &str) -> String {
        match self.0 {
            1 => format!("Aktualizácia {v}"),
            2 => format!("Aktualizace {v}"),
            3 => format!("更新 {v}"),
            4 => format!("更新 {v}"),
            _ => format!("Update {v}"),
        }
    }

    pub fn install(self, v: &str) -> String {
        match self.0 {
            1 => format!("Stiahnuť a nainštalovať {v}"),
            2 => format!("Stáhnout a nainstalovat {v}"),
            3 => format!("下载并安装 {v}"),
            4 => format!("下載並安裝 {v}"),
            _ => format!("Download and install {v}"),
        }
    }

    pub fn downloading(self, v: &str, pct: Option<u32>) -> String {
        let p = pct.map(|p| format!(" {p} %")).unwrap_or_default();
        match self.0 {
            1 => format!("Sťahujem verziu {v}…{p}"),
            2 => format!("Stahuji verzi {v}…{p}"),
            3 => format!("正在下载 {v}…{p}"),
            4 => format!("正在下載 {v}…{p}"),
            _ => format!("Downloading version {v}…{p}"),
        }
    }

    pub fn installed(self, v: &str) -> String {
        match self.0 {
            1 => format!("Verzia {v} je nainštalovaná. Reštartujte spúšťač."),
            2 => format!("Verze {v} je nainstalována. Restartujte spouštěč."),
            3 => format!("已安装版本 {v}。请重启启动器。"),
            4 => format!("已安裝版本 {v}。請重新啟動啟動器。"),
            _ => format!("Version {v} is installed. Restart the launcher."),
        }
    }

    pub fn update_fail(self, f: &Fail) -> String {
        match f {
            Fail::NoReleases => self
                .pick([
                    "No release has been published yet.",
                    "Zatiaľ nie je zverejnené žiadne vydanie.",
                    "Zatím není zveřejněné žádné vydání.",
                    "尚未发布任何版本。",
                    "尚未發布任何版本。",
                ])
                .into(),
            Fail::Network(e) => match self.0 {
                1 => format!("Aktualizácie sa nedajú zistiť: {e}"),
                2 => format!("Aktualizace nelze zjistit: {e}"),
                3 => format!("无法检查更新：{e}"),
                4 => format!("無法檢查更新：{e}"),
                _ => format!("Could not reach the releases: {e}"),
            },
            Fail::NoArchive(p) => match self.0 {
                1 => format!("Nové vydanie nemá balík pre tento systém ({p})."),
                2 => format!("Nové vydání nemá balík pro tento systém ({p})."),
                3 => format!("新版本没有适用于此系统的安装包（{p}）。"),
                4 => format!("新版本沒有適用於此系統的安裝包（{p}）。"),
                _ => format!("The new release has no package for this system ({p})."),
            },
            Fail::NoChecksum => self
                .pick([
                    "The release's package has no checksum, so it is not installed.",
                    "Balík vydania nemá kontrolný súčet, preto sa neinštaluje.",
                    "Balík vydání nemá kontrolní součet, proto se neinstaluje.",
                    "该安装包没有校验和，因此不会安装。",
                    "該安裝包沒有校驗和，因此不會安裝。",
                ])
                .into(),
            Fail::Checksum => self
                .pick([
                    "The download is damaged (checksum differs); nothing was changed.",
                    "Stiahnutý súbor je poškodený (iný kontrolný súčet); nič sa nezmenilo.",
                    "Stažený soubor je poškozený (jiný kontrolní součet); nic se nezměnilo.",
                    "下载的文件已损坏（校验和不符）；未做任何更改。",
                    "下載的檔案已損壞（校驗和不符）；未做任何變更。",
                ])
                .into(),
            Fail::Install(e) => match self.0 {
                1 => format!("Inštalácia zlyhala: {e}"),
                2 => format!("Instalace selhala: {e}"),
                3 => format!("安装失败：{e}"),
                4 => format!("安裝失敗：{e}"),
                _ => format!("The install failed: {e}"),
            },
        }
    }

    /// The system folder dialog's prompt.
    pub fn choose_folder(self, warmonger: bool) -> &'static str {
        if warmonger {
            self.pick([
                "Choose the State of War Warmonger folder",
                "Vyberte priečinok State of War Warmonger",
                "Vyberte složku State of War Warmonger",
                "选择 State of War Warmonger 文件夹",
                "選擇 State of War Warmonger 資料夾",
            ])
        } else {
            self.pick([
                "Choose the State of War folder",
                "Vyberte priečinok State of War",
                "Vyberte složku State of War",
                "选择 State of War 文件夹",
                "選擇 State of War 資料夾",
            ])
        }
    }

    /// NATIVE, the resolution that follows the window (the game's own word for it, `Tr::native`).
    pub fn native(self) -> &'static str {
        self.pick(["Native", "Natívne", "Nativní", "原生", "原生"])
    }

    /// A setting's label and, for this port's switches, a one-line explanation (the game's `Tr::option_rows`).
    pub fn field(self, f: Field) -> (&'static str, &'static str) {
        let label = match f {
            Field::Resolution => self.pick(["Resolution", "Rozlíšenie", "Rozlišení", "分辨率", "解析度"]),
            Field::Clouds => self.pick(["Clouds", "Oblaky", "Mraky", "云层", "雲層"]),
            Field::Radar => self.pick(["Radar", "Radar", "Radar", "雷达", "雷達"]),
            Field::Quake => self.pick(["Screen shake", "Otrasy obrazovky", "Otřesy obrazovky", "屏幕震动", "螢幕震動"]),
            Field::Vsync => self.pick(["Vertical sync", "Vertikálna synchronizácia", "Vertikální synchronizace", "垂直同步", "垂直同步"]),
            Field::HwCursor => self.pick(["Hardware cursor", "Hardvérový kurzor", "Hardwarový kurzor", "硬件光标", "硬體游標"]),
            Field::KeysScroll => self.pick(["Keyboard scroll", "Posun klávesmi", "Posun klávesami", "键盘滚动", "鍵盤捲動"]),
            Field::MouseScroll => self.pick(["Mouse scroll", "Posun myšou", "Posun myší", "鼠标滚动", "滑鼠捲動"]),
            Field::Music | Field::MusicVolume => self.pick(["Music", "Hudba", "Hudba", "音乐", "音樂"]),
            Field::Speech | Field::SpeechVolume => self.pick(["Speech", "Reč", "Řeč", "语音", "語音"]),
            Field::Effects | Field::EffectsVolume => self.pick(["Effects", "Efekty", "Efekty", "音效", "音效"]),
            Field::Extra(k) => return self.extra(k),
        };
        (label, "")
    }

    fn extra(self, k: usize) -> (&'static str, &'static str) {
        const T: [[(&str, &str); 8]; 5] = [
            [
                ("Enhanced textures", "Smooths sprites and the ground when you zoom in"),
                ("Soft shadows", "Blurs the edges of shadows"),
                ("Contact shadows", "Dark halo where buildings and units meet the ground"),
                ("Explosion glow", "Explosions and muzzle flashes light the ground"),
                ("Turret shadows", "Every turret casts a shadow (original: only A.A.)"),
                ("Enhanced sound", "Cleaner, smoother effects, speech and music"),
                ("Turret range", "Shows how far a turret reaches while you place it"),
                ("Map zoom", "Zoom with the mouse wheel (original: fixed scale)"),
            ],
            [
                ("Vylepšené textúry", "Vyhladí jednotky, budovy a terén pri priblížení"),
                ("Mäkké tiene", "Rozmaže okraje tieňov"),
                ("Kontaktné tiene", "Tmavý lem, kde sa budovy a jednotky dotýkajú zeme"),
                ("Žiara výbuchov", "Výbuchy a záblesky výstrelov osvetľujú terén"),
                ("Tiene veží", "Každá veža vrhá tieň (pôvodne len protilietadlová)"),
                ("Vylepšený zvuk", "Čistejšie a plynulejšie efekty, reč a hudba"),
                ("Dosah veží", "Pri umiestňovaní veže ukáže, kam dostrelí"),
                ("Priblíženie mapy", "Približovanie kolieskom myši (pôvodne pevná mierka)"),
            ],
            [
                ("Vylepšené textury", "Vyhladí jednotky, budovy a terén při přiblížení"),
                ("Měkké stíny", "Rozmaže okraje stínů"),
                ("Kontaktní stíny", "Tmavý lem, kde se budovy a jednotky dotýkají země"),
                ("Záře výbuchů", "Výbuchy a záblesky výstřelů osvětlují terén"),
                ("Stíny věží", "Každá věž vrhá stín (původně jen protiletadlová)"),
                ("Vylepšený zvuk", "Čistší a plynulejší efekty, řeč a hudba"),
                ("Dosah věží", "Při umísťování věže ukáže, kam dostřelí"),
                ("Přiblížení mapy", "Přibližování kolečkem myši (původně pevné měřítko)"),
            ],
            [
                ("增强纹理", "放大时平滑单位、建筑和地面"),
                ("柔和阴影", "模糊阴影边缘"),
                ("接触阴影", "建筑和单位与地面接触处的暗晕"),
                ("爆炸光晕", "爆炸和枪口火光照亮地面"),
                ("炮塔阴影", "每座炮塔都投下阴影（原版仅防空炮塔）"),
                ("增强音效", "更清晰、更平滑的音效、语音和音乐"),
                ("炮塔射程", "放置炮塔时显示其射程"),
                ("地图缩放", "用鼠标滚轮缩放（原版为固定比例）"),
            ],
            [
                ("增強紋理", "放大時平滑單位、建築和地面"),
                ("柔和陰影", "模糊陰影邊緣"),
                ("接觸陰影", "建築和單位與地面接觸處的暗暈"),
                ("爆炸光暈", "爆炸和槍口火光照亮地面"),
                ("砲塔陰影", "每座砲塔都投下陰影（原版僅防空砲塔）"),
                ("增強音效", "更清晰、更平滑的音效、語音和音樂"),
                ("砲塔射程", "放置砲塔時顯示其射程"),
                ("地圖縮放", "用滑鼠滾輪縮放（原版為固定比例）"),
            ],
        ];
        T[self.0].get(k).copied().unwrap_or(("", ""))
    }

    pub fn message(self, m: &Message) -> String {
        match m {
            Message::None => String::new(),
            Message::Set(name) => match self.0 {
                1 => format!("{name}: nastavené."),
                2 => format!("{name}: nastaveno."),
                3 => format!("已设置 {name}。"),
                4 => format!("已設定 {name}。"),
                _ => format!("{name} set."),
            },
            Message::NotTheFolder(dir, name) => match self.0 {
                1 => format!("{dir} nevyzerá ako priečinok {name}."),
                2 => format!("{dir} nevypadá jako složka {name}."),
                3 => format!("{dir} 看起来不是 {name} 的文件夹。"),
                4 => format!("{dir} 看起來不是 {name} 的資料夾。"),
                _ => format!("{dir} does not look like the {name} folder."),
            },
            Message::NothingFound => self
                .pick([
                    "Nothing new found; use Locate game or drop the game's folder here.",
                    "Nič nové sa nenašlo; použite Nájsť hru alebo sem pretiahnite priečinok hry.",
                    "Nic nového se nenašlo; použijte Najít hru nebo sem přetáhněte složku hry.",
                    "没有找到新的安装；请使用“定位游戏”或将游戏文件夹拖到这里。",
                    "沒有找到新的安裝；請使用「定位遊戲」或將遊戲資料夾拖到這裡。",
                ])
                .to_string(),
            Message::Found(n) => match self.0 {
                1 => format!("Nájdené inštalácie: {n}."),
                2 => format!("Nalezené instalace: {n}."),
                3 => format!("找到 {n} 个安装。"),
                4 => format!("找到 {n} 個安裝。"),
                _ => format!("Found {n} install(s)."),
            },
            Message::Missing(name) => match self.0 {
                1 => format!("{name}: chýbajú herné súbory; nájdite hru znova."),
                2 => format!("{name}: chybí herní soubory; najděte hru znovu."),
                3 => format!("{name}：缺少游戏文件；请重新定位游戏。"),
                4 => format!("{name}：缺少遊戲檔案；請重新定位遊戲。"),
                _ => format!("{name}: the game files are missing; locate the game again."),
            },
            Message::SettingsReset => self
                .pick([
                    "Game settings are back to their defaults.",
                    "Nastavenia hry sú znova predvolené.",
                    "Nastavení hry je znovu výchozí.",
                    "游戏设置已恢复默认。",
                    "遊戲設定已恢復預設。",
                ])
                .to_string(),
            Message::SaveFailed(e) => match self.0 {
                1 => format!("Nastavenia sa nedajú uložiť: {e}"),
                2 => format!("Nastavení nelze uložit: {e}"),
                3 => format!("无法保存设置：{e}"),
                4 => format!("無法儲存設定：{e}"),
                _ => format!("Could not save the settings: {e}"),
            },
            Message::DropFolder => self
                .pick([
                    "Drop the game's folder, not a file.",
                    "Pretiahnite priečinok hry, nie súbor.",
                    "Přetáhněte složku hry, ne soubor.",
                    "请拖入游戏文件夹，而不是文件。",
                    "請拖入遊戲資料夾，而不是檔案。",
                ])
                .to_string(),
            Message::GameMissing(path) => match self.0 {
                1 => format!("{path} sa nenašiel; zostavte ho príkazom cargo build --release -p opensow-game"),
                2 => format!("{path} nenalezen; sestavte ho příkazem cargo build --release -p opensow-game"),
                3 => format!("找不到 {path}；请用 cargo build --release -p opensow-game 编译"),
                4 => format!("找不到 {path}；請用 cargo build --release -p opensow-game 編譯"),
                _ => format!("{path} not found; build it with cargo build --release -p opensow-game"),
            },
            Message::Raw(s) => s.clone(),
        }
    }

    /// Every sentence of the launcher's own (none of them may stay English in a translation).
    pub fn all(self) -> Vec<String> {
        let mut v: Vec<String> = [
            self.window_title(),
            self.launcher(),
            self.home(),
            self.settings(),
            self.game_files_label(),
            self.auto_detect(),
            self.play(),
            self.ready(),
            self.not_located(),
            self.files_missing(),
            self.optional(),
            self.tagline(),
            self.your_games(),
            self.locate_first(),
            self.find_games(),
            self.subtitle(false),
            self.subtitle(true),
            self.locate(),
            self.change_folder(),
            self.files_found(),
            self.files_missing_long(),
            self.not_located_long(),
            self.folder_hint(false),
            self.folder_hint(true),
            self.needs_base(),
            self.campaign_note(),
            self.tab_extras(),
            self.graphics(),
            self.stored_in(),
            self.reset(),
            self.extras_note(),
            self.choose_folder(false),
            self.choose_folder(true),
            self.native(),
            self.updates(),
            self.check_at_start(),
            self.check_now(),
            self.release_notes(),
            self.restart(),
            self.checking(),
            self.up_to_date(),
        ]
        .map(String::from)
        .to_vec();
        v.extend([
            self.version("1.0.0"),
            self.available("1.0.0", "N"),
            self.update_badge("1.0.0"),
            self.install("1.0.0"),
            self.downloading("1.0.0", Some(5)),
            self.installed("1.0.0"),
        ]);
        for f in [Fail::NoReleases, Fail::Network("e".into()), Fail::NoArchive("p".into()), Fail::NoChecksum, Fail::Checksum, Fail::Install("e".into())] {
            v.push(self.update_fail(&f));
        }
        for k in 0..8 {
            let (a, b) = self.field(Field::Extra(k));
            v.extend([a.to_string(), b.to_string()]);
        }
        for m in [
            Message::Set("X".into()),
            Message::NotTheFolder("D".into(), "X".into()),
            Message::NothingFound,
            Message::Found(2),
            Message::Missing("X".into()),
            Message::SettingsReset,
            Message::SaveFailed("e".into()),
            Message::DropFolder,
            Message::GameMissing("p".into()),
        ] {
            v.push(self.message(&m));
        }
        v
    }

    /// Short words that may be the same as English in some languages (Jazyk, Radar, Hra, Zvuk); for the font check.
    pub fn words(self) -> Vec<&'static str> {
        let mut v = vec![self.language(), self.tab_game(), self.sound()];
        for f in [
            Field::Resolution,
            Field::Clouds,
            Field::Radar,
            Field::Quake,
            Field::Vsync,
            Field::HwCursor,
            Field::KeysScroll,
            Field::MouseScroll,
            Field::Music,
            Field::Speech,
            Field::Effects,
        ] {
            v.push(self.field(f).0);
        }
        v
    }
}

/// The note the launcher shows: what the last action did. Kept as what happened, so it shows in the page's language also after
/// the language changes.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Message {
    #[default]
    None,
    /// A game folder was set (the game's name).
    Set(String),
    /// A folder that is not the game's: (folder, the game's name).
    NotTheFolder(String, String),
    NothingFound,
    Found(usize),
    /// Play was pressed but the game's files are gone (the game's name).
    Missing(String),
    SettingsReset,
    SaveFailed(String),
    DropFolder,
    GameMissing(String),
    /// Text from elsewhere (the system's error, `--message`).
    Raw(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_has_every_text_and_they_differ_from_english() {
        let en = Tx::english().all();
        for l in Language::SUPPORTED {
            let t = Tx::new(l).all();
            assert_eq!(t.len(), en.len());
            assert!(t.iter().all(|s| !s.trim().is_empty()), "{l:?}");
            if l != Language::English {
                let same: Vec<_> = t.iter().zip(&en).filter(|(a, b)| a == b).collect();
                assert!(same.is_empty(), "{l:?}: still English: {same:?}");
            }
            assert!(Tx::new(l).words().iter().all(|w| !w.is_empty()));
        }
        assert_eq!(Tx::new(Language::Polish), Tx::english(), "languages the port does not offer read English");
    }
}
