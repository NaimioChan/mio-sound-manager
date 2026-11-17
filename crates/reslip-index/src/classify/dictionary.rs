//! Category taxonomy: category → subcategory → the words that select it.
//!
//! Authored for this project against a real 271k-file sample library, not
//! transcribed from another implementation. The vocabulary reflects what that
//! library actually contains and how it names things: pack folder names are
//! marketing (`Epic.Stock.Media.Hybrid.Game.Sounds`) and are ignored by the
//! matcher, filenames are the description (`ESM_SPG_fx_servo_metal_latch_02`),
//! and the Epic Stock Media packs carry a Chinese folder name that states the
//! pack theme (`【蒸汽朋克游戏音效】`), which is why there is a separate Chinese
//! hint list.
//!
//! Do not hand-edit: `tools/gen-category-dict.py` generates this file. The
//! vocabulary, the weights and the tokenizer are tuned together, so a word
//! added here without re-running the corpus check is a guess.
//!
//! `__bare__` is the category-level entry: the words that name the category
//! itself (`drum`, `foley`, `weapon`) rather than one of its subcategories. A
//! subcategory hit outranks a bare-category hit on a tie, so the bare entry only
//! decides files that carry no more specific word.

use super::CategoryDefinition;

pub static CATEGORY_DICTIONARY: &[CategoryDefinition] = &[
    CategoryDefinition {
        category: "Drums",
        subcategory: None,
        keywords: &[
            "drum", "drums",
        ],
        hints: &[
            "鼓组", "打击鼓组",
        ],
        phrases: &[
            "drum loop", "top loop", "break beat",
        ],
    },
    CategoryDefinition {
        category: "Drums",
        subcategory: Some("Kicks"),
        keywords: &[
            "kick", "kicks", "bd", "bassdrum", "808sub", "subkick", "thump", "drumkick",
        ],
        hints: &[
            "底鼓", "大鼓", "踢鼓",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Drums",
        subcategory: Some("Snares & Claps"),
        keywords: &[
            "snare", "snares", "snr", "rimshot", "rimshots", "rim", "clap", "claps",
            "handsnap",
        ],
        hints: &[
            "军鼓", "小鼓", "拍手", "掌声",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Drums",
        subcategory: Some("Hats & Cymbals"),
        keywords: &[
            "hat", "hats", "hihat", "hihats", "hh", "cymbal", "cymbals", "ride", "rides",
            "crash", "crashes", "tambourine", "tambs",
        ],
        hints: &[
            "踩镲", "镲片", "吊镲", "沙锤", "铃鼓",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Drums",
        subcategory: Some("Percussion"),
        keywords: &[
            "perc", "percussion", "percs", "conga", "congas", "bongo", "bongos", "djembe",
            "tabla", "tablas", "cajon", "darbuka", "timbale", "timbales", "woodblock",
            "cowbell", "agogo", "shaker", "shakers", "guiro", "cabasa", "claves", "maracas",
            "udu",
        ],
        hints: &[
            "打击乐", "手鼓", "木鱼", "牛铃",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Drums",
        subcategory: Some("Toms"),
        keywords: &[
            "tom", "toms", "floortom", "racktom", "rototom",
        ],
        hints: &[
            "通鼓", "嗵鼓",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Drums",
        subcategory: Some("Fills & Rolls"),
        keywords: &[
            "fill", "fills", "filler", "drumfill", "drumroll", "rudiment", "rudiments",
        ],
        hints: &[
            "加花",
        ],
        phrases: &[
            "drum fill", "snare roll", "drum roll",
        ],
    },
    CategoryDefinition {
        category: "Drums",
        subcategory: Some("Breakbeats"),
        keywords: &[
            "breakbeat", "breakbeats", "amenbreak", "thinkbreak",
        ],
        hints: &[
            "碎拍",
        ],
        phrases: &[
            "amen break", "funky drummer",
        ],
    },
    CategoryDefinition {
        category: "Drums",
        subcategory: Some("Drum Loops"),
        keywords: &[
            "drumloop", "drumloops", "toploop", "toploops", "beatloop", "drumgroove",
        ],
        hints: &[
            "鼓循环",
        ],
        phrases: &[
            "drum loop", "top loop",
        ],
    },
    CategoryDefinition {
        category: "Drums",
        subcategory: Some("Drum Kits"),
        keywords: &[
            "drumkit", "drumkits",
        ],
        hints: &[
            "套鼓",
        ],
        phrases: &[
            "drum kit",
        ],
    },
    CategoryDefinition {
        category: "Bass",
        subcategory: Some("Sub & 808"),
        keywords: &[
            "sub", "subbass", "808s", "bass808", "sublayer", "sinebass",
        ],
        hints: &[
            "次低音", "超低音",
        ],
        phrases: &[
            "808 bass", "sub bass",
        ],
    },
    CategoryDefinition {
        category: "Bass",
        subcategory: Some("Growls & Reeses"),
        keywords: &[
            "reese", "wobble", "wobbles", "yoi", "screechbass",
        ],
        hints: &[
            "低音咆哮",
        ],
        phrases: &[
            "bass growl",
        ],
    },
    CategoryDefinition {
        category: "Bass",
        subcategory: Some("Bass One Shots"),
        keywords: &[
            "bass", "basses", "bassline", "basslines", "bassshot", "bassone",
        ],
        hints: &[
            "贝斯", "电贝司", "低音",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Bass",
        subcategory: Some("Bass Loops"),
        keywords: &[
            "bassloop", "bassloops", "basstops",
        ],
        hints: &[
            "贝斯循环",
        ],
        phrases: &[
            "bass loop", "bass loops",
        ],
    },
    CategoryDefinition {
        category: "Synths",
        subcategory: Some("Leads"),
        keywords: &[
            "lead", "leads", "leadline", "supersaw", "squarelead", "pulseline",
        ],
        hints: &[
            "主音", "主奏",
        ],
        phrases: &[
            "synth lead",
        ],
    },
    CategoryDefinition {
        category: "Synths",
        subcategory: Some("Pads & Drones"),
        keywords: &[
            "pad", "pads", "warmpad", "stringpad", "drone", "drones", "sustained", "texture",
            "textures", "soundscape", "soundscapes",
        ],
        hints: &[
            "音垫", "铺底", "持续音",
        ],
        phrases: &[
            "synth pad",
        ],
    },
    CategoryDefinition {
        category: "Synths",
        subcategory: Some("Plucks & Arps"),
        keywords: &[
            "pluck", "plucks", "arp", "arps", "arpeggio", "arpeggios", "arpeggiated",
        ],
        hints: &[
            "拨弦", "琶音",
        ],
        phrases: &[
            "synth pluck",
        ],
    },
    CategoryDefinition {
        category: "Synths",
        subcategory: Some("Stabs & Chords"),
        keywords: &[
            "stab", "stabs", "chord", "chords", "chordsynth",
        ],
        hints: &[
            "和弦", "短促和弦",
        ],
        phrases: &[
            "synth stab",
        ],
    },
    CategoryDefinition {
        category: "Synths",
        subcategory: Some("Synth Loops"),
        keywords: &[
            "synthloop", "synthloops", "melodicloop",
        ],
        hints: &[
            "合成循环",
        ],
        phrases: &[
            "synth loop", "synth loops",
        ],
    },
    CategoryDefinition {
        category: "Synths",
        subcategory: Some("Synth One Shots"),
        keywords: &[
            "synth", "synths", "synthesizer", "synthesizers", "wavetable", "serum", "massive",
            "sylenth", "vital",
        ],
        hints: &[
            "合成器", "合成",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Keys & Mallets",
        subcategory: Some("Piano"),
        keywords: &[
            "piano", "pianos", "grandpiano", "upright",
        ],
        hints: &[
            "钢琴",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Keys & Mallets",
        subcategory: Some("Electric Piano & Organ"),
        keywords: &[
            "rhodes", "wurlitzer", "wurli", "organ", "organs", "clav", "clavinet", "epiano",
            "keys",
        ],
        hints: &[
            "电钢", "风琴", "电钢琴",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Keys & Mallets",
        subcategory: Some("Mallets & Bells"),
        keywords: &[
            "mallet", "mallets", "marimba", "xylophone", "glockenspiel", "vibraphone", "vibes",
            "celesta", "kalimba", "chimes", "bell", "bells", "handpan", "hangdrum", "gong",
            "musicbox",
        ],
        hints: &[
            "马林巴", "木琴", "钟琴", "风铃", "八音盒",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Guitars",
        subcategory: Some("Acoustic Guitar"),
        keywords: &[
            "acousticguitar", "steelstring", "nylonstring",
        ],
        hints: &[
            "木吉他", "原声吉他",
        ],
        phrases: &[
            "acoustic guitar",
        ],
    },
    CategoryDefinition {
        category: "Guitars",
        subcategory: Some("Electric Guitar"),
        keywords: &[
            "guitar", "guitars", "gtr", "riff", "riffs", "strum", "palmute", "chug",
        ],
        hints: &[
            "吉他", "电吉他",
        ],
        phrases: &[
            "electric guitar",
        ],
    },
    CategoryDefinition {
        category: "Guitars",
        subcategory: Some("Guitar Loops"),
        keywords: &[
            "guitarloop", "guitarloops",
        ],
        hints: &[
            "吉他循环",
        ],
        phrases: &[
            "guitar loop", "guitar loops",
        ],
    },
    CategoryDefinition {
        category: "Strings & Winds",
        subcategory: Some("Strings"),
        keywords: &[
            "violin", "violins", "viola", "violas", "cello", "cellos", "contrabass",
            "doublebass", "pizzicato", "spiccato", "tremolo", "strings",
        ],
        hints: &[
            "弦乐", "小提琴", "中提琴", "大提琴", "二胡", "古筝", "琵琶", "箜篌", "马头琴", "古琴",
        ],
        phrases: &[
            "string section",
        ],
    },
    CategoryDefinition {
        category: "Strings & Winds",
        subcategory: Some("Brass"),
        keywords: &[
            "trumpet", "trumpets", "trombone", "trombones", "tuba", "flugelhorn", "cornet",
            "frenchhorn", "brasssection",
        ],
        hints: &[
            "铜管", "小号", "长号", "圆号", "唢呐",
        ],
        phrases: &[
            "brass section",
        ],
    },
    CategoryDefinition {
        category: "Strings & Winds",
        subcategory: Some("Woodwinds"),
        keywords: &[
            "flute", "flutes", "clarinet", "clarinets", "oboe", "bassoon", "sax", "saxophone",
            "piccolo", "recorder",
        ],
        hints: &[
            "长笛", "单簧管", "双簧管", "萨克斯", "笛子",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Strings & Winds",
        subcategory: Some("World Instruments"),
        keywords: &[
            "duduk", "erhu", "guqin", "pipa", "konghou", "zheng", "koto", "shakuhachi",
            "shamisen", "sitar", "oud", "bouzouki", "didgeridoo", "yangqin",
        ],
        hints: &[
            "陶笛", "扬琴", "三味线", "排箫",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Strings & Winds",
        subcategory: Some("Harps"),
        keywords: &[
            "harp", "harps", "harpsichord", "lyre",
        ],
        hints: &[
            "竖琴",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Vocals",
        subcategory: Some("Vocal One Shots"),
        keywords: &[
            "vocal", "vocals", "vox", "voice", "acapella", "acappella", "acapellas",
        ],
        hints: &[
            "人声", "演唱",
        ],
        phrases: &[
            "vocal shot",
        ],
    },
    CategoryDefinition {
        category: "Vocals",
        subcategory: Some("Chops"),
        keywords: &[
            "chop", "chops", "vocalchop", "choppedvox",
        ],
        hints: &[
            "人声切片",
        ],
        phrases: &[
            "vocal chop",
        ],
    },
    CategoryDefinition {
        category: "Vocals",
        subcategory: Some("Choirs & Chants"),
        keywords: &[
            "choir", "choirs", "choral", "chant", "chants", "chanting", "mantra", "hymn",
        ],
        hints: &[
            "合唱", "圣歌", "吟唱",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Vocals",
        subcategory: Some("Spoken & Dialogue"),
        keywords: &[
            "spoken", "dialogue", "dialog", "narration", "narrator", "announcer", "whisper",
            "whispers",
        ],
        hints: &[
            "对白", "旁白", "语音",
        ],
        phrases: &[
            "stock line", "spoken word",
        ],
    },
    CategoryDefinition {
        category: "Vocals",
        subcategory: Some("Rap & Adlibs"),
        keywords: &[
            "rap", "rapping", "adlib", "adlibs", "freestyle",
        ],
        hints: &[
            "说唱",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Vocals",
        subcategory: Some("Game Characters"),
        keywords: &[
            "grunt", "grunts", "effort", "yell", "yells", "shout", "shouts", "scream",
            "screams", "battlecry", "warcry", "deathcry", "vocalization", "vocalizations",
            "emote", "emotes",
        ],
        hints: &[
            "角色配音", "呐喊", "惨叫", "尖叫",
        ],
        phrases: &[
            "character vocalization",
        ],
    },
    CategoryDefinition {
        category: "Vocals",
        subcategory: Some("Vocal Loops"),
        keywords: &[
            "vocalloop", "vocalloops",
        ],
        hints: &[
            "人声循环",
        ],
        phrases: &[
            "vocal loop", "vocal loops",
        ],
    },
    CategoryDefinition {
        category: "Music",
        subcategory: None,
        keywords: &[
            "music", "beat", "beats",
        ],
        hints: &[
            "歌曲", "曲目",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Music",
        subcategory: Some("Full Tracks"),
        keywords: &[
            "fulltrack", "song", "songs", "soundtrack", "cue",
        ],
        hints: &[
            "完整曲目", "配乐",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Music",
        subcategory: Some("Stems"),
        keywords: &[
            "stem", "multitrack", "instrumental",
        ],
        hints: &[
            "分轨",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Music",
        subcategory: Some("Musical Loops"),
        keywords: &[
            "musicloop", "musicloops", "progression",
        ],
        hints: &[
            "配乐循环",
        ],
        phrases: &[
            "music loop",
        ],
    },
    CategoryDefinition {
        category: "Foley",
        subcategory: None,
        keywords: &[
            "foley",
        ],
        hints: &[
            "拟音",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Foley",
        subcategory: Some("Footsteps"),
        keywords: &[
            "footstep", "footsteps", "step", "boot", "boots", "shoe", "shoes", "heel", "heels",
            "sneaker", "slipper",
        ],
        hints: &[
            "脚步", "走路", "靴子",
        ],
        phrases: &[
            "foot steps",
        ],
    },
    CategoryDefinition {
        category: "Foley",
        subcategory: Some("Cloth & Movement"),
        keywords: &[
            "cloth", "clothes", "fabric", "rustle", "rustles", "jacket", "sleeve", "zipper",
            "velcro", "apparel",
        ],
        hints: &[
            "衣服", "布料", "摩擦",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Foley",
        subcategory: Some("Body & Hands"),
        keywords: &[
            "hand", "hands", "finger", "fingers", "fingerclick", "breath", "breathing", "sigh",
            "swallow",
        ],
        hints: &[
            "身体", "拍打",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Foley",
        subcategory: Some("Doors & Locks"),
        keywords: &[
            "door", "doors", "drawer", "drawers", "latch", "latches", "lock", "locks",
            "handle", "knob", "hinge", "gate", "cabinet",
        ],
        hints: &[
            "舱门", "电子门", "大门", "门锁", "抽屉", "飞船舱门",
        ],
        phrases: &[
            "door open", "door close", "door slam",
        ],
    },
    CategoryDefinition {
        category: "Foley",
        subcategory: Some("Kitchen & Food"),
        keywords: &[
            "kitchen", "cook", "cooking", "food", "eating", "bite", "chew", "sip", "kettle",
            "microwave", "fridge", "oven", "cutlery", "dish", "plate", "chopping", "frying",
            "boiling",
        ],
        hints: &[
            "厨房", "烹饪", "吃饭", "餐具",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Foley",
        subcategory: Some("Household & Office"),
        keywords: &[
            "household", "office", "desk", "chair", "vacuum", "broom", "mop", "cleaning",
            "wash", "washing", "toothbrush", "shower", "toilet", "towel", "faucet", "flush",
        ],
        hints: &[
            "家居", "办公", "打扫",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Foley",
        subcategory: Some("Objects & Props"),
        keywords: &[
            "prop", "props", "object", "objects", "pickup", "dropobject", "grab", "fumble",
            "toolbox", "toolbag",
        ],
        hints: &[
            "道具", "物品", "拿起", "放下",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Foley",
        subcategory: Some("Paper & Packaging"),
        keywords: &[
            "paper", "cardboard", "bag", "bags", "box", "boxes", "package", "parcel", "wrap",
            "wrapping", "crinkle", "crinkles",
        ],
        hints: &[
            "纸张", "包装", "塑料袋",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Foley",
        subcategory: Some("Sports & Toys"),
        keywords: &[
            "ball", "bounce", "skid", "skateboard", "toy", "toys", "playground", "swing",
        ],
        hints: &[
            "玩具", "皮球",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Foley",
        subcategory: Some("Water Foley"),
        keywords: &[
            "splash", "splashes", "drip", "drips", "dropwater", "pour", "gurgle", "bubble",
            "bubbles", "slosh", "squish",
        ],
        hints: &[
            "水滴", "水花", "倒水", "水声",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Impacts",
        subcategory: Some("Metal"),
        keywords: &[
            "metal", "metallic", "clang", "clangs", "clank", "iron", "steel", "aluminum",
            "aluminium", "anvil", "pipe", "pipes", "chain", "chains",
        ],
        hints: &[
            "金属", "铁链", "钢制",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Impacts",
        subcategory: Some("Wood"),
        keywords: &[
            "wood", "wooden", "timber", "plank", "planks", "trunk", "log", "clatter",
        ],
        hints: &[
            "木头", "木质", "木材",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Impacts",
        subcategory: Some("Glass"),
        keywords: &[
            "glass", "glasses", "shatter", "shatters", "smash", "crystal", "bottle", "bottles",
        ],
        hints: &[
            "玻璃", "碎玻璃",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Impacts",
        subcategory: Some("Plastic & Ceramic"),
        keywords: &[
            "plastic", "ceramic", "porcelain", "clay", "pottery", "terracotta",
        ],
        hints: &[
            "塑料", "陶瓷",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Impacts",
        subcategory: Some("Stone & Concrete"),
        keywords: &[
            "stone", "stones", "concrete", "cement", "brick", "bricks", "gravel", "rubble",
            "debris",
        ],
        hints: &[
            "石头", "混凝土", "砖块", "碎屑",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Impacts",
        subcategory: Some("Hard Hits"),
        keywords: &[
            "hit", "hits", "impact", "impacts", "slam", "slams", "thud", "thuds", "whack",
            "bash", "knock", "pound",
        ],
        hints: &[
            "撞击", "冲击", "打击", "碰撞", "碎裂",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Impacts",
        subcategory: Some("Punches & Body Blows"),
        keywords: &[
            "punch", "punches", "slap", "smack", "fist", "jab", "uppercut", "blow", "blows",
        ],
        hints: &[
            "拳击", "殴打", "拳打脚踢", "打斗",
        ],
        phrases: &[
            "punch hit",
        ],
    },
    CategoryDefinition {
        category: "Impacts",
        subcategory: Some("Crash & Collapse"),
        keywords: &[
            "crumble", "collapse", "crashing", "wreck", "wreckage",
        ],
        hints: &[
            "崩塌", "废墟", "破坏", "炸毁",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Impacts",
        subcategory: Some("Sub Drops"),
        keywords: &[
            "subdrop", "subdrops", "subbomb",
        ],
        hints: &[
            "低频坠落",
        ],
        phrases: &[
            "sub drop",
        ],
    },
    CategoryDefinition {
        category: "Weapons",
        subcategory: None,
        keywords: &[
            "weapon", "weapons", "weaponry",
        ],
        hints: &[
            "兵器",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Weapons",
        subcategory: Some("Firearms"),
        keywords: &[
            "gun", "guns", "firearm", "firearms", "gunshot", "gunshots", "shoot", "shooting",
            "rifle", "rifles", "carbine", "pistol", "pistols", "handgun", "handguns",
            "revolver", "revolvers", "shotgun", "shotguns", "smg", "submachinegun",
            "machinegun", "minigun", "sniper", "magnum", "deagle", "glock", "beretta", "uzi",
            "ak47", "akm", "m16", "m4", "m4a1", "m249", "mp5", "musket", "flintlock",
            "blunderbuss", "derringer",
        ],
        hints: &[
            "步枪", "手枪", "冲锋枪", "机枪", "狙击枪", "霰弹枪", "枪械", "枪声",
        ],
        phrases: &[
            "assault rifle", "sniper rifle", "machine gun",
        ],
    },
    CategoryDefinition {
        category: "Weapons",
        subcategory: Some("Ammo & Handling"),
        keywords: &[
            "bullet", "bullets", "shell", "shells", "casing", "casings", "brass", "cartridge",
            "cartridges", "ammo", "ammunition", "magazine", "magazines", "clip", "reload",
            "reloading", "racking", "bolt", "chamber", "chambering", "cock", "cocking",
            "trigger", "safety", "suppressor", "silencer", "muzzle", "recoil", "ricochet",
            "dryfire",
        ],
        hints: &[
            "子弹", "弹壳", "换弹", "上膛", "枪栓", "弹匣", "消音", "枪械操作",
        ],
        phrases: &[
            "shell casing", "dry fire", "muzzle flash",
        ],
    },
    CategoryDefinition {
        category: "Weapons",
        subcategory: Some("Melee & Blades"),
        keywords: &[
            "sword", "swords", "blade", "blades", "katana", "katanas", "sabre", "saber",
            "rapier", "longsword", "broadsword", "cutlass", "scimitar", "dagger", "daggers",
            "knife", "knives", "machete", "axe", "axes", "hatchet", "hammer", "mace", "spear",
            "spears", "pike", "halberd", "staff", "baton", "club", "flail", "sheath",
            "sheathing", "sharpen", "whet",
        ],
        hints: &[
            "刀剑", "匕首", "长剑", "长矛", "武士刀", "斧头", "铁锤",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Weapons",
        subcategory: Some("Bows & Arrows"),
        keywords: &[
            "bow", "bows", "arrow", "arrows", "crossbow", "crossbows", "quiver", "drawbow",
        ],
        hints: &[
            "弓箭", "弩箭",
        ],
        phrases: &[
            "bow draw",
        ],
    },
    CategoryDefinition {
        category: "Weapons",
        subcategory: Some("Explosives"),
        keywords: &[
            "grenade", "grenades", "bomb", "bombs", "dynamite", "tnt", "mortar", "artillery",
            "mine", "landmine", "rocket", "rocketlauncher", "warhead", "missile", "missiles",
        ],
        hints: &[
            "手榴弹", "炸弹", "火箭弹",
        ],
        phrases: &[
            "grenade launcher", "rocket launcher",
        ],
    },
    CategoryDefinition {
        category: "Weapons",
        subcategory: Some("Sci-Fi Weapons"),
        keywords: &[
            "laser", "lasers", "blaster", "blasters", "plasma", "phaser", "phasers", "railgun",
            "ioncannon", "disintegrator", "pulser",
        ],
        hints: &[
            "激光枪", "激光武器", "科幻武器", "等离子武器",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Weapons",
        subcategory: Some("Casing Drops"),
        keywords: &[
            "bulletdrop", "brassdrop", "casingdrop",
        ],
        hints: &[
            "弹壳落地",
        ],
        phrases: &[
            "bullet drop", "shell drop",
        ],
    },
    CategoryDefinition {
        category: "Explosions & Fire",
        subcategory: Some("Blasts"),
        keywords: &[
            "explosion", "explosions", "blast", "blasts", "detonation", "detonations", "boom",
            "booms", "eruption", "blastwave", "shockwave",
        ],
        hints: &[
            "爆炸", "爆破", "冲击波",
        ],
        phrases: &[
            "explosion blast",
        ],
    },
    CategoryDefinition {
        category: "Explosions & Fire",
        subcategory: Some("Fire & Burning"),
        keywords: &[
            "fire", "flames", "flame", "burning", "burn", "ignite", "ignition", "sizzle",
            "sizzles", "crackle", "crackling", "ember", "embers", "torch", "campfire",
            "wildfire",
        ],
        hints: &[
            "燃烧", "火焰", "火元素",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Explosions & Fire",
        subcategory: Some("Fuse & Debris"),
        keywords: &[
            "fuse", "sparks", "shrapnel", "incoming", "whistlebomb",
        ],
        hints: &[
            "引信",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Vehicles",
        subcategory: None,
        keywords: &[
            "vehicle", "vehicles",
        ],
        hints: &[
            "载具", "交通工具",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Vehicles",
        subcategory: Some("Cars & Trucks"),
        keywords: &[
            "car", "cars", "truck", "trucks", "engine", "engines", "motor", "motors",
            "exhaust", "tire", "tires", "tyre", "horn", "horns", "brake", "brakes",
            "transmission", "turbo", "clutch",
        ],
        hints: &[
            "汽车", "引擎", "车辆", "轮胎",
        ],
        phrases: &[
            "car engine",
        ],
    },
    CategoryDefinition {
        category: "Vehicles",
        subcategory: Some("Motorcycles & Bikes"),
        keywords: &[
            "motorcycle", "motorcycles", "motorbike", "motorbikes", "scooter", "scooters",
            "moped", "bicycle", "bicycles", "bike", "bikes",
        ],
        hints: &[
            "摩托", "自行车",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Vehicles",
        subcategory: Some("Aircraft"),
        keywords: &[
            "plane", "planes", "aircraft", "airplane", "jet", "jets", "helicopter",
            "helicopters", "heli", "rotor", "propeller", "propellers", "turbine", "jetengine",
            "uav",
        ],
        hints: &[
            "飞机", "直升机", "螺旋桨",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Vehicles",
        subcategory: Some("Watercraft"),
        keywords: &[
            "boat", "boats", "ship", "ships", "yacht", "sailboat", "submarine", "canoe",
            "kayak", "ferry", "motorboat",
        ],
        hints: &[
            "轮船", "潜艇", "帆船",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Vehicles",
        subcategory: Some("Rail"),
        keywords: &[
            "train", "trains", "rail", "rails", "railway", "locomotive", "tram", "subway",
            "metro", "crossing",
        ],
        hints: &[
            "火车", "列车", "地铁",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Vehicles",
        subcategory: Some("Sci-Fi Craft"),
        keywords: &[
            "spaceship", "spaceships", "starship", "hovercar", "speeder", "warpdrive",
            "thruster", "thrusters",
        ],
        hints: &[
            "飞船", "悬浮车", "飞船引擎",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Machinery",
        subcategory: None,
        keywords: &[
            "mechanical", "machine", "machines", "machinery",
        ],
        hints: &[
            "机械", "机器",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Machinery",
        subcategory: Some("Motors & Servos"),
        keywords: &[
            "servo", "servos", "actuator", "actuators", "spindle", "electricmotor",
            "motorized",
        ],
        hints: &[
            "伺服", "电机", "马达",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Machinery",
        subcategory: Some("Gears & Ratchets"),
        keywords: &[
            "gear", "gears", "cog", "cogs", "ratchet", "ratchets", "sprocket", "winch",
            "pulley", "crank", "flywheel",
        ],
        hints: &[
            "齿轮", "棘轮", "链条",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Machinery",
        subcategory: Some("Hydraulics & Pneumatics"),
        keywords: &[
            "hydraulic", "hydraulics", "pneumatic", "pneumatics", "piston", "pistons", "valve",
            "valves", "compressor", "steam", "vent", "airrelease",
        ],
        hints: &[
            "液压", "气动", "气压",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Machinery",
        subcategory: Some("Tools & Power"),
        keywords: &[
            "drill", "driller", "saw", "chainsaw", "grinder", "sander", "wrench", "hammering",
            "screwdriver", "jackhammer", "nailgun", "welder", "welding", "industrial",
        ],
        hints: &[
            "电钻", "扳手", "工具", "电锯",
        ],
        phrases: &[
            "power tool",
        ],
    },
    CategoryDefinition {
        category: "Machinery",
        subcategory: Some("Switches & Mechanisms"),
        keywords: &[
            "switch", "switches", "lever", "coil", "gadget", "device",
        ],
        hints: &[
            "开关", "机械装置",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Machinery",
        subcategory: Some("Clocks & Ticking"),
        keywords: &[
            "clock", "clocks", "tick", "ticking", "tock", "metronome", "timer", "countdown",
            "clockwork", "pendulum",
        ],
        hints: &[
            "时钟", "计时",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Nature",
        subcategory: None,
        keywords: &[
            "nature",
        ],
        hints: &[
            "自然",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Nature",
        subcategory: Some("Wind"),
        keywords: &[
            "wind", "winds", "breeze", "gust", "gusts", "gale",
        ],
        hints: &[
            "风声", "风暴", "风系", "狂风",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Nature",
        subcategory: Some("Water & Rain"),
        keywords: &[
            "water", "rain", "raining", "river", "stream", "creek", "brook", "waterfall",
            "ocean", "sea", "waves", "surf", "lake", "pond", "puddle", "underwater",
        ],
        hints: &[
            "水元素", "流水", "河流", "海浪", "雨水", "雨声",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Nature",
        subcategory: Some("Thunder & Storms"),
        keywords: &[
            "thunder", "thunders", "thunderstorm", "storm", "storms", "lightning", "hail",
            "blizzard", "hurricane", "tornado",
        ],
        hints: &[
            "雷声", "雷鸣", "闪电", "暴雨", "暴风",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Nature",
        subcategory: Some("Animals & Birds"),
        keywords: &[
            "dog", "dogs", "cat", "cats", "horse", "horses", "cow", "cows", "sheep", "goat",
            "pig", "pigs", "wolf", "wolves", "fox", "bear", "deer", "bird", "birds", "crow",
            "crows", "owl", "owls", "seagull", "pigeon", "chicken", "rooster", "duck", "geese",
            "goose", "frog", "frogs", "cricket", "crickets", "bee", "bees", "mosquito",
            "snake", "snakes",
        ],
        hints: &[
            "动物", "鸟叫", "狼嚎", "虫鸣",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Nature",
        subcategory: Some("Vegetation & Earth"),
        keywords: &[
            "forest", "jungle", "tree", "trees", "leaves", "leaf", "grass", "bush", "bushes",
            "branch", "branches", "dirt", "soil", "mud", "sand", "earthquake",
        ],
        hints: &[
            "森林", "树叶", "土壤",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Ambience",
        subcategory: None,
        keywords: &[
            "ambience", "ambiance", "ambient", "atmosphere", "atmospheres",
        ],
        hints: &[
            "环境音", "氛围", "环境声",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Ambience",
        subcategory: Some("Nature Beds"),
        keywords: &[
            "ambiencebed",
        ],
        hints: &[
            "野外", "荒野", "生态",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Ambience",
        subcategory: Some("Urban & City"),
        keywords: &[
            "city", "urban", "street", "streets", "traffic", "crowd", "crowds", "market",
            "plaza", "airport", "station", "platform", "siren", "sirens", "construction",
        ],
        hints: &[
            "城市", "街道", "人群", "交通",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Ambience",
        subcategory: Some("Interior & Room Tone"),
        keywords: &[
            "interior", "roomtone", "hallway", "corridor", "lobby", "restaurant", "cafe",
            "church", "classroom", "hospital", "ventilation", "conditioner", "fluorescent",
        ],
        hints: &[
            "室内", "房间", "走廊", "办公室", "机房",
        ],
        phrases: &[
            "room tone",
        ],
    },
    CategoryDefinition {
        category: "Ambience",
        subcategory: Some("Industrial"),
        keywords: &[
            "factory", "warehouse", "docksite", "harbor", "generator", "engineroom",
        ],
        hints: &[
            "工厂", "工业", "车间",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Ambience",
        subcategory: Some("Underwater"),
        keywords: &[
            "submerged", "subaquatic", "aquatic",
        ],
        hints: &[
            "水下", "海底",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Ambience",
        subcategory: Some("Sci-Fi & Space"),
        keywords: &[
            "space", "cosmic", "cosmos", "galactic", "nebula", "orbital", "future", "cyber",
            "laboratory", "reactor",
        ],
        hints: &[
            "太空", "宇宙", "实验室",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Creatures",
        subcategory: Some("Monsters & Beasts"),
        keywords: &[
            "monster", "monsters", "creature", "creatures", "beast", "beasts", "zombie",
            "zombies", "undead", "ghoul", "goblin", "orc", "troll", "ogre", "werewolf",
            "wendigo", "yeti", "bigfoot",
        ],
        hints: &[
            "怪物", "怪兽", "僵尸", "狼人", "雪怪", "丧尸", "魔兽", "兽人", "巨魔", "哥布林", "史前",
        ],
        phrases: &[
            "monster roar", "creature growl", "beast growl",
        ],
    },
    CategoryDefinition {
        category: "Creatures",
        subcategory: Some("Dragons & Reptiles"),
        keywords: &[
            "dragon", "dragons", "drake", "wyvern", "reptile", "reptiles", "lizard", "lizards",
            "serpent", "basilisk",
        ],
        hints: &[
            "巨龙", "蜥蜴",
        ],
        phrases: &[
            "dragon roar",
        ],
    },
    CategoryDefinition {
        category: "Creatures",
        subcategory: Some("Aliens & Robots"),
        keywords: &[
            "alien", "aliens", "extraterrestrial", "robot", "robots", "droid", "droids",
        ],
        hints: &[
            "外星", "异形", "机器人",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Creatures",
        subcategory: Some("Humanoids & Spirits"),
        keywords: &[
            "demon", "demons", "devil", "devils", "imp", "angel", "angels", "spirit",
            "spirits", "ghost", "ghosts", "wraith", "phantom", "apparition", "poltergeist",
        ],
        hints: &[
            "恶魔", "幽灵", "鬼魂", "精灵", "亡灵",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Creatures",
        subcategory: Some("Gore & Violence"),
        keywords: &[
            "gore", "bloody", "blood", "flesh", "bone", "bones", "dismember", "squelch",
            "splatter", "butcher", "carnage",
        ],
        hints: &[
            "血腥", "内脏", "血肉", "流血",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Magic",
        subcategory: Some("Spells & Casts"),
        keywords: &[
            "magic", "magical", "spell", "spells", "cast", "casting", "conjure", "conjuring",
            "incantation", "enchant", "enchantment", "sorcery", "wizard", "sorcerer", "witch",
            "warlock", "rune", "runes", "mystic", "mystical", "arcane",
        ],
        hints: &[
            "魔法", "法术", "施法", "咒语", "巫术", "符文", "魔法师", "巫师", "奥术",
        ],
        phrases: &[
            "spell cast", "magic cast",
        ],
    },
    CategoryDefinition {
        category: "Magic",
        subcategory: Some("Enchantments & Buffs"),
        keywords: &[
            "buff", "buffs", "aura", "blessing", "blessed", "holy", "divine", "heal",
            "healing", "luck", "charm", "shield", "summon", "summoning", "portal", "portals",
        ],
        hints: &[
            "祝福", "增益", "光环", "神圣", "传送门", "治疗",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Magic",
        subcategory: Some("Elemental Magic"),
        keywords: &[
            "elemental", "fireball", "fireballs", "frost", "freezing", "energy",
        ],
        hints: &[
            "元素", "火球", "冰霜", "元素魔法", "魔法能量",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Magic",
        subcategory: Some("Dark & Cursed"),
        keywords: &[
            "curse", "cursed", "hex", "hexed", "plague", "blight", "doom", "occult", "ritual",
            "sacrifice",
        ],
        hints: &[
            "诅咒", "黑暗", "仪式",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Magic",
        subcategory: Some("Potions & Items"),
        keywords: &[
            "potion", "potions", "elixir", "flask", "vial", "scroll", "scrolls", "wand",
            "wands", "talisman", "amulet",
        ],
        hints: &[
            "药水", "水晶", "卷轴",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Sci-Fi & Tech",
        subcategory: None,
        keywords: &[
            "scifi", "sci", "cyberpunk",
        ],
        hints: &[
            "科幻", "未来科技", "高科技",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Sci-Fi & Tech",
        subcategory: Some("Energy & Plasma"),
        keywords: &[
            "beam", "beams", "pulse", "pulses", "charging", "overload", "forcefield",
            "deflector",
        ],
        hints: &[
            "等离子", "脉冲", "护盾", "能量",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Sci-Fi & Tech",
        subcategory: Some("Beams & Rays"),
        keywords: &[
            "raygun", "taser", "zap", "zaps",
        ],
        hints: &[
            "射线", "激光束",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Sci-Fi & Tech",
        subcategory: Some("Computers & Data"),
        keywords: &[
            "computer", "computers", "data", "digital", "cybernetic", "hack", "hacking",
            "encryption", "download", "upload", "matrix", "terminal",
        ],
        hints: &[
            "电脑", "数据", "数字", "黑客", "计算机",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Sci-Fi & Tech",
        subcategory: Some("Teleport & Warp"),
        keywords: &[
            "teleport", "teleportation", "warping", "jumpgate", "hyperspace", "stargate",
            "wormhole",
        ],
        hints: &[
            "传送", "跃迁",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Sci-Fi & Tech",
        subcategory: Some("Holograms & Scanners"),
        keywords: &[
            "hologram", "holograms", "holographic", "scanner", "scan", "scanning", "sonar",
            "radar", "beacon",
        ],
        hints: &[
            "全息", "扫描仪", "雷达",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Sci-Fi & Tech",
        subcategory: Some("Circuit & Electric"),
        keywords: &[
            "circuit", "circuitry", "voltage", "current", "electric", "electrical",
            "electricity", "spark", "arc", "arcing", "buzz", "buzzing", "electrocute",
        ],
        hints: &[
            "电流", "电路", "电压", "电气",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "UI & Game",
        subcategory: None,
        keywords: &[
            "ui", "interface", "hud",
        ],
        hints: &[
            "用户界面", "交互界面",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "UI & Game",
        subcategory: Some("Clicks & Buttons"),
        keywords: &[
            "click", "clicks", "tap", "taps", "press", "button", "buttons", "cursor", "hover",
            "select", "toggle",
        ],
        hints: &[
            "按钮", "点击", "电子按钮",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "UI & Game",
        subcategory: Some("Notifications & Alerts"),
        keywords: &[
            "beep", "beeps", "blip", "blips", "notify", "notification", "notifications",
            "alert", "alerts", "alarm", "message", "tone", "ding",
        ],
        hints: &[
            "通知", "提示音", "报警",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "UI & Game",
        subcategory: Some("Menus & Navigation"),
        keywords: &[
            "menu", "menus", "navigate", "navigation", "swipe", "tab", "inventory", "map",
            "quest", "confirm", "cancel",
        ],
        hints: &[
            "菜单", "导航", "取消",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "UI & Game",
        subcategory: Some("Rewards & Progress"),
        keywords: &[
            "coin", "coins", "reward", "rewards", "achievement", "achievements", "unlock",
            "levelup", "victory", "win", "success", "star", "stars", "score", "loot",
            "treasure",
        ],
        hints: &[
            "奖励", "金币", "升级", "胜利", "得分", "解锁",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "UI & Game",
        subcategory: Some("Errors & Warnings"),
        keywords: &[
            "error", "errors", "wrong", "fail", "failed", "failure", "deny", "denied",
            "reject", "warning", "warnings", "forbidden", "invalid", "critical",
        ],
        hints: &[
            "错误", "失败", "警告",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "UI & Game",
        subcategory: Some("Typing & Keyboards"),
        keywords: &[
            "typing", "type", "keystroke", "keystrokes", "mouseclick", "touchscreen",
            "gesture",
        ],
        hints: &[
            "打字",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "UI & Game",
        subcategory: Some("Card & Board Games"),
        keywords: &[
            "card", "cards", "deck", "shuffle", "dice", "boardgame", "chess", "casino", "slot",
        ],
        hints: &[
            "卡牌", "纸牌", "骰子", "棋盘",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Transitions",
        subcategory: Some("Risers & Build-ups"),
        keywords: &[
            "riser", "risers", "rise", "rising", "uplift", "uplifter", "buildup", "sweep",
            "sweeps", "swell", "tension",
        ],
        hints: &[
            "上升", "渐强",
        ],
        phrases: &[
            "build up", "riser",
        ],
    },
    CategoryDefinition {
        category: "Transitions",
        subcategory: Some("Downlifters & Falls"),
        keywords: &[
            "downlifter", "downlifters", "fall", "faller", "falls", "descent", "descend",
            "downsweep",
        ],
        hints: &[
            "下降", "渐弱",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Transitions",
        subcategory: Some("Whooshes & Sweeps"),
        keywords: &[
            "whoosh", "whooshes", "swoosh", "swooshes", "swish", "passby", "flyby",
            "transition", "transitions",
        ],
        hints: &[
            "呼啸", "掠过", "过渡", "转场",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Transitions",
        subcategory: Some("Reverses & Tape Stops"),
        keywords: &[
            "reverse", "reverses", "reversed", "backwards", "tape", "tapestop", "spinback",
            "backspin",
        ],
        hints: &[
            "倒放", "磁带停止",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Transitions",
        subcategory: Some("Glitches & Stutters"),
        keywords: &[
            "glitch", "glitches", "glitched", "stutter", "stutters", "repeater", "buffer",
            "skip", "skips",
        ],
        hints: &[
            "故障", "卡顿", "干扰",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Transitions",
        subcategory: Some("Cinematic Hits"),
        keywords: &[
            "braam", "braams", "stinger", "stingers",
        ],
        hints: &[
            "预告片", "电影感",
        ],
        phrases: &[
            "cinematic hit", "trailer hit",
        ],
    },
    CategoryDefinition {
        category: "Horror",
        subcategory: Some("Stingers & Scares"),
        keywords: &[
            "horror", "scary", "scare", "scares", "eerie", "dread", "creepy", "sinister",
            "unsettling", "disturbing",
        ],
        hints: &[
            "恐怖", "惊悚", "毛骨悚然", "诡异",
        ],
        phrases: &[
            "jump scare",
        ],
    },
    CategoryDefinition {
        category: "Horror",
        subcategory: Some("Tension & Drones"),
        keywords: &[
            "suspense", "ominous", "foreboding", "pulsing", "darkdrone",
        ],
        hints: &[
            "紧张", "悬疑", "低鸣",
        ],
        phrases: &[],
    },
];
