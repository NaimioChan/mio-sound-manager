//! Category taxonomy: category → subcategory → the words that select it.
//!
//! Authored for this project against a real 271k-file sample library, not
//! transcribed from another implementation. The vocabulary reflects what that
//! library actually contains and how it names things: pack folder names are
//! marketing (`Epic.Stock.Media.Hybrid.Game.Sounds`) and are ignored by the
//! matcher, filenames are the description (`ESM_SPG_fx_servo_metal_latch_02`),
//! and the Epic Stock Media packs carry a Chinese folder name that states the
//! pack theme, which is deliberately not read: see `tools/taxonomy.py`.
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Drums",
        subcategory: Some("Snares & Claps"),
        keywords: &[
            "snare", "snares", "snr", "rimshot", "rimshots", "rim", "clap", "claps",
            "handsnap",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Drums",
        subcategory: Some("Percussion"),
        keywords: &[
            "perc", "percussion", "percussive", "percs", "conga", "congas", "bongo", "bongos",
            "djembe", "tabla", "tablas", "cajon", "darbuka", "timbale", "timbales",
            "woodblock", "cowbell", "agogo", "shaker", "shakers", "guiro", "cabasa", "claves",
            "maracas", "udu",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Drums",
        subcategory: Some("Toms"),
        keywords: &[
            "tom", "toms", "floortom", "racktom", "rototom",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Drums",
        subcategory: Some("Fills & Rolls"),
        keywords: &[
            "fill", "fills", "filler", "drumfill", "drumroll", "rudiment", "rudiments",
        ],
        phrases: &[
            "drum fill", "snare roll", "drum roll",
        ],
    },
    CategoryDefinition {
        category: "Drums",
        subcategory: Some("Breakbeats"),
        keywords: &[
            "break", "breakbeat", "breakbeats", "amenbreak", "thinkbreak",
        ],
        phrases: &[
            "amen break", "funky drummer", "drum break",
        ],
    },
    CategoryDefinition {
        category: "Drums",
        subcategory: Some("Drum Loops"),
        keywords: &[
            "drumloop", "drumloops", "toploop", "toploops", "beatloop", "drumgroove",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Bass",
        subcategory: Some("Bass Loops"),
        keywords: &[
            "bassloop", "bassloops", "basstops",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Keys & Mallets",
        subcategory: Some("Piano"),
        keywords: &[
            "piano", "pianos", "grandpiano", "upright",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Guitars",
        subcategory: Some("Acoustic Guitar"),
        keywords: &[
            "acousticguitar", "steelstring", "nylonstring",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Strings & Winds",
        subcategory: Some("World Instruments"),
        keywords: &[
            "duduk", "erhu", "guqin", "pipa", "konghou", "zheng", "koto", "shakuhachi",
            "shamisen", "sitar", "oud", "bouzouki", "didgeridoo", "yangqin",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Strings & Winds",
        subcategory: Some("Harps"),
        keywords: &[
            "harp", "harps", "harpsichord", "lyre",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Vocals",
        subcategory: Some("Vocal One Shots"),
        keywords: &[
            "vocal", "vocals", "vox", "voice", "acapella", "acappella", "acapellas",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Vocals",
        subcategory: Some("Spoken & Dialogue"),
        keywords: &[
            "spoken", "dialogue", "dialog", "narration", "narrator", "announcer", "whisper",
            "whispers",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Vocals",
        subcategory: Some("Game Characters"),
        keywords: &[
            "grunt", "grunts", "effort", "yell", "yells", "shout", "shouts", "scream",
            "screams", "battlecry", "warcry", "deathcry", "vocalization", "vocalizations",
            "vocalizing", "emote", "emotes",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Music",
        subcategory: Some("Full Tracks"),
        keywords: &[
            "fulltrack", "song", "songs", "soundtrack", "cue",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Music",
        subcategory: Some("Stems"),
        keywords: &[
            "stem", "multitrack", "instrumental",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Music",
        subcategory: Some("Musical Loops"),
        keywords: &[
            "musicloop", "musicloops", "progression", "melodyloop", "songstarter",
        ],
        phrases: &[
            "music loop", "melody loop",
        ],
    },
    CategoryDefinition {
        category: "Foley",
        subcategory: None,
        keywords: &[
            "foley",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Foley",
        subcategory: Some("Body & Hands"),
        keywords: &[
            "hand", "hands", "finger", "fingers", "fingerclick", "breath", "breathing", "sigh",
            "swallow",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Foley",
        subcategory: Some("Household & Office"),
        keywords: &[
            "household", "office", "desk", "chair", "vacuum", "broom", "mop", "cleaning",
            "wash", "washing", "toothbrush", "shower", "toilet", "towel", "faucet", "flush",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Foley",
        subcategory: Some("Paper & Packaging"),
        keywords: &[
            "paper", "cardboard", "bag", "bags", "box", "boxes", "package", "parcel", "wrap",
            "wrapping", "crinkle", "crinkles",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Foley",
        subcategory: Some("Sports & Toys"),
        keywords: &[
            "ball", "bounce", "skid", "skateboard", "toy", "toys", "playground", "swing",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Impacts",
        subcategory: Some("Metal"),
        keywords: &[
            "metal", "metallic", "clang", "clangs", "clank", "iron", "steel", "aluminum",
            "aluminium", "anvil", "pipe", "pipes", "chain", "chains",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Impacts",
        subcategory: Some("Wood"),
        keywords: &[
            "wood", "wooden", "timber", "plank", "planks", "trunk", "log", "clatter",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Impacts",
        subcategory: Some("Glass"),
        keywords: &[
            "glass", "glasses", "shatter", "shatters", "smash", "crystal", "bottle", "bottles",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Impacts",
        subcategory: Some("Plastic & Ceramic"),
        keywords: &[
            "plastic", "ceramic", "porcelain", "clay", "pottery", "terracotta",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Impacts",
        subcategory: Some("Hard Hits"),
        keywords: &[
            "hit", "hits", "impact", "impacts", "slam", "slams", "thud", "thuds", "whack",
            "bash", "knock", "pound", "strike", "strikes",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Impacts",
        subcategory: Some("Punches & Body Blows"),
        keywords: &[
            "punch", "punches", "slap", "smack", "fist", "jab", "uppercut", "blow", "blows",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Impacts",
        subcategory: Some("Sub Drops"),
        keywords: &[
            "subdrop", "subdrops", "subbomb",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Weapons",
        subcategory: Some("Bows & Arrows"),
        keywords: &[
            "bow", "bows", "arrow", "arrows", "crossbow", "crossbows", "quiver", "drawbow",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Weapons",
        subcategory: Some("Casing Drops"),
        keywords: &[
            "bulletdrop", "brassdrop", "casingdrop",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Explosions & Fire",
        subcategory: Some("Fuse & Debris"),
        keywords: &[
            "fuse", "sparks", "shrapnel", "incoming", "whistlebomb",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Vehicles",
        subcategory: None,
        keywords: &[
            "vehicle", "vehicles",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Vehicles",
        subcategory: Some("Watercraft"),
        keywords: &[
            "boat", "boats", "ship", "ships", "yacht", "sailboat", "submarine", "canoe",
            "kayak", "ferry", "motorboat",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Vehicles",
        subcategory: Some("Sci-Fi Craft"),
        keywords: &[
            "spaceship", "spaceships", "starship", "hovercar", "speeder", "warpdrive",
            "thruster", "thrusters",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Machinery",
        subcategory: None,
        keywords: &[
            "mechanical", "machine", "machines", "machinery", "mech", "mechs",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Machinery",
        subcategory: Some("Motors & Servos"),
        keywords: &[
            "servo", "servos", "actuator", "actuators", "spindle", "electricmotor",
            "motorized", "bot", "bots",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Machinery",
        subcategory: Some("Hydraulics & Pneumatics"),
        keywords: &[
            "hydraulic", "hydraulics", "pneumatic", "pneumatics", "piston", "pistons", "valve",
            "valves", "compressor", "steam", "vent", "airrelease",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Machinery",
        subcategory: Some("Clocks & Ticking"),
        keywords: &[
            "clock", "clocks", "tick", "ticking", "tock", "metronome", "timer", "countdown",
            "clockwork", "pendulum",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Nature",
        subcategory: None,
        keywords: &[
            "nature",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Nature",
        subcategory: Some("Wind"),
        keywords: &[
            "wind", "winds", "breeze", "gust", "gusts", "gale",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Nature",
        subcategory: Some("Thunder & Storms"),
        keywords: &[
            "thunder", "thunders", "thunderstorm", "storm", "storms", "lightning", "hail",
            "blizzard", "hurricane", "tornado",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Nature",
        subcategory: Some("Animals & Birds"),
        keywords: &[
            "dog", "dogs", "cat", "cats", "horse", "horses", "cow", "cows", "sheep", "goat",
            "pig", "pigs", "wolf", "wolves", "bear", "deer", "bird", "birds", "birdsong",
            "crow", "crows", "owl", "owls", "seagull", "pigeon", "chicken", "rooster", "duck",
            "geese", "goose", "frog", "frogs", "cricket", "crickets", "bee", "bees",
            "mosquito", "snake", "snakes", "animals", "animal",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Ambience",
        subcategory: None,
        keywords: &[
            "ambience", "ambiance", "ambient", "atmosphere", "atmospheres",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Ambience",
        subcategory: Some("Nature Beds"),
        keywords: &[
            "ambiencebed",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Ambience",
        subcategory: Some("Interior & Room Tone"),
        keywords: &[
            "interior", "roomtone", "hallway", "corridor", "lobby", "restaurant", "cafe",
            "church", "classroom", "hospital", "ventilation", "conditioner", "fluorescent",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Ambience",
        subcategory: Some("Underwater"),
        keywords: &[
            "submerged", "subaquatic", "aquatic",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Creatures",
        subcategory: Some("Humanoids & Spirits"),
        keywords: &[
            "demon", "demons", "devil", "devils", "imp", "angel", "angels", "spirit",
            "spirits", "ghost", "ghosts", "wraith", "phantom", "apparition", "poltergeist",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Magic",
        subcategory: Some("Elemental Magic"),
        keywords: &[
            "elemental", "fireball", "fireballs", "frost", "freezing", "energy",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Magic",
        subcategory: Some("Potions & Items"),
        keywords: &[
            "potion", "potions", "elixir", "flask", "vial", "scroll", "scrolls", "wand",
            "wands", "talisman", "amulet",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Sci-Fi & Tech",
        subcategory: None,
        keywords: &[
            "scifi", "sci", "cyberpunk",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Sci-Fi & Tech",
        subcategory: Some("Beams & Rays"),
        keywords: &[
            "raygun", "taser", "zap", "zaps",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Sci-Fi & Tech",
        subcategory: Some("Teleport & Warp"),
        keywords: &[
            "teleport", "teleportation", "warping", "jumpgate", "hyperspace", "stargate",
            "wormhole",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Sci-Fi & Tech",
        subcategory: Some("Circuit & Electric"),
        keywords: &[
            "circuit", "circuitry", "voltage", "current", "electric", "electrical",
            "electricity", "spark", "arc", "arcing", "buzz", "buzzing", "electrocute",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "UI & Game",
        subcategory: None,
        keywords: &[
            "ui", "interface", "hud",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "UI & Game",
        subcategory: Some("Notifications & Alerts"),
        keywords: &[
            "beep", "beeps", "blip", "blips", "notify", "notification", "notifications",
            "alert", "alerts", "alarm", "message", "tone", "ding",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "UI & Game",
        subcategory: Some("Errors & Warnings"),
        keywords: &[
            "error", "errors", "wrong", "fail", "failed", "failure", "deny", "denied",
            "reject", "warning", "warnings", "forbidden", "invalid", "critical",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "UI & Game",
        subcategory: Some("Card & Board Games"),
        keywords: &[
            "card", "cards", "deck", "shuffle", "dice", "boardgame", "chess", "casino", "slot",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Transitions",
        subcategory: Some("Risers & Build-ups"),
        keywords: &[
            "riser", "risers", "rise", "rising", "uplift", "uplifter", "upsweep", "upsweeps",
            "buildup", "sweep", "sweeps", "swell", "tension",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Transitions",
        subcategory: Some("Whooshes & Sweeps"),
        keywords: &[
            "whoosh", "whooshes", "swoosh", "swooshes", "swish", "passby", "flyby",
            "transition", "transitions",
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
        phrases: &[],
    },
    CategoryDefinition {
        category: "Transitions",
        subcategory: Some("Glitches & Stutters"),
        keywords: &[
            "glitch", "glitches", "glitched", "stutter", "stutters", "repeater", "buffer",
            "skip", "skips",
        ],
        phrases: &[],
    },
    CategoryDefinition {
        category: "Transitions",
        subcategory: Some("Cinematic Hits"),
        keywords: &[
            "braam", "braams", "stinger", "stingers",
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
        phrases: &[],
    },
];
