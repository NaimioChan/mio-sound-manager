"""Hand-authored projection from UCS 8.2.1 onto this project's taxonomy.

UCS is a sound librarian's index: 82 top categories, 753 Category/SubCategory
entries, and roughly ten thousand synonyms. This project's 22 categories are a
producer's taxonomy, so the projection is lossy on purpose — the point is to
put UCS's vocabulary to work inside the categories the UI already offers, not
to reproduce UCS's tree.

Three tables:

`TOP` gives every UCS top category a default target in our taxonomy, or `None`
to drop the whole branch. `SUB` overrides individual entries. `(cat, None)`
means "the category level" — the generated keyword lands on the `__bare__`
entry, which is what an unqualified `impact` should do.

Only entries that name a sound class we actually have are mapped. `ARCHIVED`
(stems, printmasters, ADR, impulse responses), `AIR` (compressed air, vacuums)
and `CHEMICALS` have no home in a sample manager's 22 and are dropped rather
than forced somewhere wrong; dropping them is visible in the generated file,
forced fits would not be.

`DROP_SYNONYMS` is the measured part. A UCS synonym is only useful if it names
the sound; the list carries English words that belong to no single class
(`one`, `shots`, `and`, `morph`) and words that only made sense inside a UCS
subcategory that has no counterpart here. Each entry there was checked against
the reference corpus and is listed with the reason it was cut, because the next
person to touch this file needs to know which measurement to redo.
"""

# UCS top category -> default target, or None to drop the branch.
TOP = {
    "AIR": None,                      # compressed air and vacuum; no class here
    "AIRCRAFT": ("Vehicles", "Aircraft"),
    "ALARMS": ("UI & Game", "Notifications & Alerts"),
    "AMBIENCE": ("Ambience", None),
    "ANIMALS": ("Nature", "Animals & Birds"),
    "ARCHIVED": None,                 # ADR, stems, printmasters, test tones
    "BEEPS": ("UI & Game", "Notifications & Alerts"),
    "BELLS": ("Keys & Mallets", "Mallets & Bells"),
    "BIRDS": ("Nature", "Animals & Birds"),
    "BOATS": ("Vehicles", "Watercraft"),
    "BULLETS": ("Weapons", "Ammo & Handling"),
    "CARTOON": ("Foley", "Objects & Props"),
    "CERAMICS": ("Impacts", "Plastic & Ceramic"),
    "CHAINS": ("Foley", "Objects & Props"),
    "CHEMICALS": None,                # acid and reactions; no class here
    "CLOCKS": ("Machinery", "Clocks & Ticking"),
    "CLOTH": ("Foley", "Cloth & Movement"),
    "COMMUNICATIONS": ("Foley", "Household & Office"),
    "COMPUTERS": ("Sci-Fi & Tech", "Computers & Data"),
    "CREATURES": ("Creatures", None),
    "CROWDS": ("Ambience", "Urban & City"),
    "DESIGNED": ("Transitions", None),
    "DESTRUCTION": ("Impacts", "Crash & Collapse"),
    "DIRT & SAND": ("Impacts", "Stone & Concrete"),
    "DOORS": ("Foley", "Doors & Locks"),
    "DRAWERS": ("Foley", "Household & Office"),
    "ELECTRICITY": ("Sci-Fi & Tech", "Circuit & Electric"),
    "EQUIPMENT": ("Machinery", "Tools & Power"),
    "EXPLOSIONS": ("Explosions & Fire", "Blasts"),
    "FARTS": ("Foley", "Body & Hands"),
    "FIGHT": ("Impacts", "Punches & Body Blows"),
    "FIRE": ("Explosions & Fire", "Fire & Burning"),
    "FIREWORKS": ("Explosions & Fire", "Blasts"),
    "FOLEY": ("Foley", None),
    "FOOD & DRINK": ("Foley", "Kitchen & Food"),
    "FOOTSTEPS": ("Foley", "Footsteps"),
    "GAMES": ("UI & Game", "Card & Board Games"),
    "GEOTHERMAL": ("Nature", "Vegetation & Earth"),
    "GLASS": ("Impacts", "Glass"),
    "GORE": ("Creatures", "Gore & Violence"),
    "GUNS": ("Weapons", "Firearms"),
    "HORNS": ("Foley", "Objects & Props"),
    "HUMAN": ("Foley", "Body & Hands"),
    "ICE": ("Impacts", "Glass"),
    "LASERS": ("Sci-Fi & Tech", "Beams & Rays"),
    "LEATHER": ("Foley", "Cloth & Movement"),
    "LIQUID & MUD": ("Foley", "Water Foley"),
    "MACHINES": ("Machinery", None),
    "MAGIC": ("Magic", None),
    "MECHANICAL": ("Machinery", "Switches & Mechanisms"),
    "METAL": ("Impacts", "Metal"),
    "MOTORS": ("Machinery", "Motors & Servos"),
    "MOVEMENT": ("Foley", "Cloth & Movement"),
    "MUSICAL": ("Music", None),
    "NATURAL DISASTER": ("Nature", None),
    "OBJECTS": ("Foley", "Objects & Props"),
    "PAPER": ("Foley", "Paper & Packaging"),
    "PLASTIC": ("Impacts", "Plastic & Ceramic"),
    "RAIN": ("Nature", "Water & Rain"),
    "ROBOTS": ("Creatures", "Aliens & Robots"),
    "ROCKS": ("Impacts", "Stone & Concrete"),
    "ROPE": ("Foley", "Objects & Props"),
    "RUBBER": ("Impacts", "Plastic & Ceramic"),
    "SCIFI": ("Sci-Fi & Tech", None),
    "SNOW": ("Impacts", "Crash & Collapse"),
    "SPORTS": ("Foley", "Sports & Toys"),
    "SWOOSHES": ("Transitions", "Whooshes & Sweeps"),
    "TOOLS": ("Machinery", "Tools & Power"),
    "TOYS": ("Foley", "Sports & Toys"),
    "TRAINS": ("Vehicles", "Rail"),
    "USER INTERFACE": ("UI & Game", None),
    "VEGETATION": ("Nature", "Vegetation & Earth"),
    "VEHICLES": ("Vehicles", "Cars & Trucks"),
    "VOICES": ("Vocals", "Game Characters"),
    "WATER": ("Foley", "Water Foley"),
    "WEATHER": ("Nature", "Thunder & Storms"),
    "WHISTLES": ("Foley", "Objects & Props"),
    "WIND": ("Nature", "Wind"),
    "WINDOWS": ("Foley", "Doors & Locks"),
    "WINGS": ("Nature", "Animals & Birds"),
    "WOOD": ("Impacts", "Wood"),
}

# (UCS top, UCS subcategory) -> target, or None to drop just that entry.
# Only the entries whose UCS home does not describe our home.
SUB = {
    # AIRCRAFT: door and interior readings belong with their hardware.
    ("AIRCRAFT", "DOOR"): ("Foley", "Doors & Locks"),
    ("AIRCRAFT", "MECHANISM"): ("Machinery", "Switches & Mechanisms"),
    ("AIRCRAFT", "RADIO CONTROLLED"): ("Vehicles", "Aircraft"),
    ("AIRCRAFT", "ROCKET"): ("Vehicles", "Aircraft"),

    # AMBIENCE: the branch splits across five of our subcategories.
    ("AMBIENCE", "AIR"): ("Ambience", "Interior & Room Tone"),
    ("AMBIENCE", "ROOM TONE"): ("Ambience", "Interior & Room Tone"),
    ("AMBIENCE", "ALPINE"): ("Ambience", "Nature Beds"),
    ("AMBIENCE", "BIRDSONG"): ("Ambience", "Nature Beds"),
    ("AMBIENCE", "DESERT"): ("Ambience", "Nature Beds"),
    ("AMBIENCE", "FARM"): ("Ambience", "Nature Beds"),
    ("AMBIENCE", "FOREST"): ("Ambience", "Nature Beds"),
    ("AMBIENCE", "GRASSLAND"): ("Ambience", "Nature Beds"),
    ("AMBIENCE", "INSECT"): ("Ambience", "Nature Beds"),
    ("AMBIENCE", "LAKESIDE"): ("Ambience", "Nature Beds"),
    ("AMBIENCE", "NAUTICAL"): ("Ambience", "Nature Beds"),
    ("AMBIENCE", "RURAL"): ("Ambience", "Nature Beds"),
    ("AMBIENCE", "SEASIDE"): ("Ambience", "Nature Beds"),
    ("AMBIENCE", "SWAMP"): ("Ambience", "Nature Beds"),
    ("AMBIENCE", "TROPICAL"): ("Ambience", "Nature Beds"),
    ("AMBIENCE", "TUNDRA"): ("Ambience", "Nature Beds"),
    ("AMBIENCE", "UNDERWATER"): ("Ambience", "Underwater"),
    ("AMBIENCE", "INDUSTRIAL"): ("Ambience", "Industrial"),
    ("AMBIENCE", "CONSTRUCTION"): ("Ambience", "Industrial"),
    ("AMBIENCE", "UNDERGROUND"): ("Ambience", "Industrial"),
    ("AMBIENCE", "HITECH"): ("Ambience", "Sci-Fi & Space"),
    ("AMBIENCE", "SCIFI"): ("Ambience", "Sci-Fi & Space"),
    ("AMBIENCE", "FANTASY"): ("Ambience", None),
    ("AMBIENCE", "DESIGNED"): ("Ambience", None),
    ("AMBIENCE", "HISTORICAL"): ("Ambience", None),
    ("AMBIENCE", "WARFARE"): ("Ambience", None),

    # ANIMALS: insects and horses still belong to the animal bed.
    ("ANIMALS", "INSECT"): ("Nature", "Animals & Birds"),

    # BULLETS: the impact half is a material hit, not ammunition handling.
    ("BULLETS", "BY IMPACT"): ("Impacts", "Metal"),
    ("BULLETS", "RICOCHET"): ("Impacts", "Metal"),
    ("BULLETS", "SHELL"): ("Weapons", "Casing Drops"),

    # CARTOON: the vehicle and horn readings are their own classes.
    ("CARTOON", "VEHICLE"): ("Vehicles", "Cars & Trucks"),
    ("CARTOON", "HORN"): ("Vehicles", "Cars & Trucks"),
    ("CARTOON", "VOCAL"): ("Vocals", "Game Characters"),
    ("CARTOON", "MUSICAL"): ("Music", "Musical Loops"),

    # CERAMICS, GLASS, ICE, PLASTIC, RUBBER, ROCKS, SNOW: the UCS material
    # pattern (break / crash / friction / handle / impact / movement / tonal)
    # collapses onto Impacts, except the fragments, which are collapses.
    ("CERAMICS", "BREAK"): ("Impacts", "Crash & Collapse"),
    ("CERAMICS", "CRASH & DEBRIS"): ("Impacts", "Crash & Collapse"),
    ("GLASS", "BREAK"): ("Impacts", "Crash & Collapse"),
    ("GLASS", "CRASH & DEBRIS"): ("Impacts", "Crash & Collapse"),
    ("ICE", "BREAK"): ("Impacts", "Crash & Collapse"),
    ("ICE", "CRASH & DEBRIS"): ("Impacts", "Crash & Collapse"),
    ("PLASTIC", "BREAK"): ("Impacts", "Crash & Collapse"),
    ("PLASTIC", "CRASH & DEBRIS"): ("Impacts", "Crash & Collapse"),
    ("RUBBER", "CRASH & DEBRIS"): ("Impacts", "Crash & Collapse"),
    ("ROCKS", "BREAK"): ("Impacts", "Crash & Collapse"),
    ("ROCKS", "CRASH & DEBRIS"): ("Impacts", "Crash & Collapse"),
    ("SNOW", "CRASH & DEBRIS"): ("Impacts", "Crash & Collapse"),

    # CHAINS: a chain breaking is a metal impact, not a prop handling.
    ("CHAINS", "BREAK"): ("Impacts", "Metal"),
    ("CHAINS", "IMPACT"): ("Impacts", "Metal"),
    ("CHAINS", "MOVEMENT"): ("Impacts", "Metal"),

    # CLOTH: rips and impacts read as cloth movement, not prop handling.
    ("CLOTH", "RIP"): ("Foley", "Cloth & Movement"),

    # COMMUNICATIONS: the electrical and data halves are Sci-Fi & Tech here.
    ("COMMUNICATIONS", "STATIC"): ("Sci-Fi & Tech", "Circuit & Electric"),
    ("COMMUNICATIONS", "TELEMETRY"): ("Sci-Fi & Tech", "Computers & Data"),
    ("COMMUNICATIONS", "AUDIO VISUAL"): ("Foley", "Household & Office"),

    # CREATURES: the branch maps almost one-to-one onto ours.
    ("CREATURES", "BEAST"): ("Creatures", "Monsters & Beasts"),
    ("CREATURES", "AQUATIC"): ("Creatures", "Monsters & Beasts"),
    ("CREATURES", "BLOB"): ("Creatures", "Monsters & Beasts"),
    ("CREATURES", "INSPECTOID"): ("Creatures", "Monsters & Beasts"),
    ("CREATURES", "MONSTER"): ("Creatures", "Monsters & Beasts"),
    ("CREATURES", "SMALL"): ("Creatures", "Monsters & Beasts"),
    ("CREATURES", "DINOSAUR"): ("Creatures", "Dragons & Reptiles"),
    ("CREATURES", "DRAGON"): ("Creatures", "Dragons & Reptiles"),
    ("CREATURES", "REPTILIAN"): ("Creatures", "Dragons & Reptiles"),
    ("CREATURES", "AVIAN"): ("Creatures", "Monsters & Beasts"),
    ("CREATURES", "ELEMENTAL"): ("Magic", "Elemental Magic"),
    ("CREATURES", "ETHEREAL"): ("Creatures", "Humanoids & Spirits"),
    ("CREATURES", "HUMANOID"): ("Creatures", "Humanoids & Spirits"),
    ("CREATURES", "SOURCE"): None,

    # CROWDS: celebration and applause are crowd beds, not choir material.
    ("CROWDS", "SINGING"): ("Vocals", "Choirs & Chants"),

    # DESIGNED: the transient vocabulary is what our Transitions branch calls
    # its own; drones and booms land where a producer looks for them.
    ("DESIGNED", "RISER"): ("Transitions", "Risers & Build-ups"),
    ("DESIGNED", "WHOOSH"): ("Transitions", "Whooshes & Sweeps"),
    ("DESIGNED", "BOOM"): ("Transitions", "Cinematic Hits"),
    ("DESIGNED", "BRAAM"): ("Transitions", "Cinematic Hits"),
    ("DESIGNED", "STINGER"): ("Transitions", "Cinematic Hits"),
    ("DESIGNED", "IMPACT"): ("Transitions", "Cinematic Hits"),
    ("DESIGNED", "DIVE"): ("Transitions", "Downlifters & Falls"),
    ("DESIGNED", "DRONE"): ("Horror", "Tension & Drones"),
    ("DESIGNED", "EERIE"): ("Horror", "Tension & Drones"),
    ("DESIGNED", "RUMBLE"): ("Horror", "Tension & Drones"),
    ("DESIGNED", "BASS"): ("Bass", "Sub & 808"),
    ("DESIGNED", "MORPH"): None,
    ("DESIGNED", "DISTORTION"): None,
    ("DESIGNED", "GRANULAR"): None,
    ("DESIGNED", "RHYTHMIC"): None,
    ("DESIGNED", "SOURCE"): None,
    ("DESIGNED", "SYNTHETIC"): None,
    ("DESIGNED", "TONAL"): None,
    ("DESIGNED", "ETHEREAL"): None,

    # DESTRUCTION: both readings are what our Crash & Collapse holds.
    ("DESTRUCTION", "COLLAPSE"): ("Impacts", "Crash & Collapse"),
    ("DESTRUCTION", "CRASH & DEBRIS"): ("Impacts", "Crash & Collapse"),

    # DOORS: the hardware and mechanism halves are machinery here.
    ("DOORS", "HYDRAULIC & PNEUMATIC"): ("Machinery", "Hydraulics & Pneumatics"),
    ("DOORS", "HARDWARE"): ("Machinery", "Switches & Mechanisms"),
    ("DOORS", "HITECH"): ("Machinery", "Switches & Mechanisms"),
    ("DOORS", "KNOCK"): ("Foley", "Doors & Locks"),
    ("DOORS", "CREAK"): ("Foley", "Doors & Locks"),

    # EQUIPMENT: the tactical and sport halves are hardware, the tack is cloth.
    ("EQUIPMENT", "BRIDLE & TACK"): ("Foley", "Cloth & Movement"),
    ("EQUIPMENT", "SPORT"): ("Foley", "Sports & Toys"),
    ("EQUIPMENT", "RECREATIONAL"): ("Foley", "Sports & Toys"),

    # EXPLOSIONS: designed versus real is not a distinction our tree makes.
    ("EXPLOSIONS", "DESIGNED"): ("Explosions & Fire", "Blasts"),
    ("EXPLOSIONS", "REAL"): ("Explosions & Fire", "Blasts"),

    # FIRE: the whoosh and gas readings belong to their own classes.
    ("FIRE", "WHOOSH"): ("Transitions", "Whooshes & Sweeps"),
    ("FIRE", "GAS"): ("Explosions & Fire", "Blasts"),

    # FOLEY: the one UCS branch that maps cleanly onto ours.
    ("FOLEY", "CLOTH"): ("Foley", "Cloth & Movement"),
    ("FOLEY", "FEET"): ("Foley", "Footsteps"),
    ("FOLEY", "HANDS"): ("Foley", "Body & Hands"),
    ("FOLEY", "PROP"): ("Foley", "Objects & Props"),

    # FOOTSTEPS: an animal's tread is still a footstep.
    ("FOOTSTEPS", "HUMAN"): ("Foley", "Footsteps"),

    # GAMES: the video half is the category itself.
    ("GAMES", "VIDEO"): ("UI & Game", None),
    ("GAMES", "ARCADE"): ("UI & Game", None),

    # GORE: the material vocabulary is our gore entry.
    ("GORE", "SOURCE"): None,

    # GUNS: mechanisms and handling are the same branch in our tree.
    ("GUNS", "MECHANISM"): ("Weapons", "Ammo & Handling"),
    ("GUNS", "HANDLE"): ("Weapons", "Ammo & Handling"),

    # HORNS: the celebration and traditional readings are props, the powered
    # ones are vehicle horns.
    ("HORNS", "POWERED"): ("Vehicles", "Cars & Trucks"),
    ("HORNS", "AIR POWERED"): ("Vehicles", "Cars & Trucks"),

    # HUMAN: everything here is a body sound.
    ("HUMAN", "BREATH"): ("Foley", "Body & Hands"),
    ("HUMAN", "SNEEZE"): ("Foley", "Body & Hands"),

    # MACHINES: the mechanism sub-branches keep their own entries.
    ("MACHINES", "FAN"): ("Machinery", "Motors & Servos"),
    ("MACHINES", "HVAC"): ("Machinery", "Motors & Servos"),
    ("MACHINES", "PUMP"): ("Machinery", "Hydraulics & Pneumatics"),
    ("MACHINES", "ELEVATOR"): ("Machinery", "Motors & Servos"),
    ("MACHINES", "ESCALATOR"): ("Machinery", "Motors & Servos"),
    ("MACHINES", "MECHANISM"): ("Machinery", "Switches & Mechanisms"),

    # MAGIC: the branch maps onto ours almost directly.
    ("MAGIC", "ANGELIC"): ("Magic", "Enchantments & Buffs"),
    ("MAGIC", "ELEMENTAL"): ("Magic", "Elemental Magic"),
    ("MAGIC", "EVIL"): ("Magic", "Dark & Cursed"),
    ("MAGIC", "POOF"): ("Magic", "Spells & Casts"),
    ("MAGIC", "SHIMMER"): ("Magic", "Enchantments & Buffs"),
    ("MAGIC", "SPELL"): ("Magic", "Spells & Casts"),

    # MECHANICAL: the whole branch is our switches and mechanisms.
    ("MECHANICAL", "GEARS"): ("Machinery", "Gears & Ratchets"),
    ("MECHANICAL", "RATCHET"): ("Machinery", "Gears & Ratchets"),
    ("MECHANICAL", "HYDRAULIC & PNEUMATIC"): ("Machinery", "Hydraulics & Pneumatics"),

    # MOTORS: the fuel and turbine halves are the same machinery.
    ("MOTORS", "COMBUSTION"): ("Machinery", "Motors & Servos"),
    ("MOTORS", "TURBINE"): ("Machinery", "Motors & Servos"),
    ("MOTORS", "SERVO"): ("Machinery", "Motors & Servos"),

    # MOVEMENT: crowd and animal movement is a body/footstep, not cloth.
    ("MOVEMENT", "CROWD"): ("Ambience", "Urban & City"),
    ("MOVEMENT", "HUMAN"): ("Foley", "Cloth & Movement"),
    ("MOVEMENT", "ANIMAL"): ("Foley", "Footsteps"),
    ("MOVEMENT", "CREATURE"): ("Foley", "Footsteps"),

    # MUSICAL: UCS's instrument tree is exactly what our music categories are.
    ("MUSICAL", "PERCUSSION"): ("Drums", "Percussion"),
    ("MUSICAL", "PERCUSSION TUNED"): ("Drums", "Percussion"),
    ("MUSICAL", "SHAKEN"): ("Drums", "Percussion"),
    ("MUSICAL", "KEYED"): ("Keys & Mallets", "Piano"),
    ("MUSICAL", "BELLS"): ("Keys & Mallets", "Mallets & Bells"),
    ("MUSICAL", "CHIME"): ("Keys & Mallets", "Mallets & Bells"),
    ("MUSICAL", "TOY"): ("Keys & Mallets", "Mallets & Bells"),
    ("MUSICAL", "BRASS"): ("Strings & Winds", "Brass"),
    ("MUSICAL", "STRINGED"): ("Strings & Winds", "Strings"),
    ("MUSICAL", "WOODWIND"): ("Strings & Winds", "Woodwinds"),
    ("MUSICAL", "PLUCKED"): ("Synths", "Plucks & Arps"),
    ("MUSICAL", "SYNTHESIZED"): ("Synths", "Synth One Shots"),
    ("MUSICAL", "CHORAL"): ("Vocals", "Choirs & Chants"),
    ("MUSICAL", "SONG & PHRASE"): ("Music", "Full Tracks"),
    ("MUSICAL", "LOOP"): ("Music", "Musical Loops"),
    ("MUSICAL", "STINGER"): ("Transitions", "Cinematic Hits"),
    ("MUSICAL", "EXPERIMENTAL"): ("Music", None),
    ("MUSICAL", "PERFORMANCE"): ("Music", None),
    ("MUSICAL", "SAMPLE"): ("Music", None),
    ("MUSICAL", "INSTRUMENT"): ("Music", None),

    # NATURAL DISASTER: the geological beds belong to nature.
    ("NATURAL DISASTER", "AVALANCHE"): ("Nature", "Vegetation & Earth"),
    ("NATURAL DISASTER", "EARTHQUAKE"): ("Nature", "Vegetation & Earth"),
    ("NATURAL DISASTER", "VOLCANO"): ("Nature", "Vegetation & Earth"),

    # OBJECTS: the packing, office and clothing halves split out.
    ("OBJECTS", "PACKAGING"): ("Foley", "Paper & Packaging"),
    ("OBJECTS", "WRITING"): ("Foley", "Paper & Packaging"),
    ("OBJECTS", "BOOK"): ("Foley", "Paper & Packaging"),
    ("OBJECTS", "HOUSEHOLD"): ("Foley", "Household & Office"),
    ("OBJECTS", "OFFICE"): ("Foley", "Household & Office"),
    ("OBJECTS", "MEDICAL"): ("Foley", "Household & Office"),
    ("OBJECTS", "FASHION"): ("Foley", "Cloth & Movement"),
    ("OBJECTS", "WHEELED"): ("Foley", "Objects & Props"),

    # PAPER: the fragments are still paper.
    ("PAPER", "RIP"): ("Foley", "Paper & Packaging"),

    # SCIFI: the vehicle, weapon and computer halves have their own homes.
    ("SCIFI", "VEHICLE"): ("Vehicles", "Sci-Fi Craft"),
    ("SCIFI", "SPACESHIP"): ("Vehicles", "Sci-Fi Craft"),
    ("SCIFI", "WEAPON"): ("Weapons", "Sci-Fi Weapons"),
    ("SCIFI", "COMPUTER"): ("Sci-Fi & Tech", "Computers & Data"),
    ("SCIFI", "ENERGY"): ("Sci-Fi & Tech", "Energy & Plasma"),
    ("SCIFI", "MACHINE"): ("Machinery", "Motors & Servos"),
    ("SCIFI", "MECHANISM"): ("Machinery", "Switches & Mechanisms"),
    ("SCIFI", "DOOR"): ("Foley", "Doors & Locks"),
    ("SCIFI", "ALARM"): ("UI & Game", "Notifications & Alerts"),

    # SPORTS: the surface and arena beds are ambiences, not props.
    ("SPORTS", "COURT"): ("Foley", "Sports & Toys"),
    ("SPORTS", "TRACK & FIELD"): ("Foley", "Sports & Toys"),

    # TRAINS: the mechanism half is machinery.
    ("TRAINS", "MECHANISM"): ("Machinery", "Switches & Mechanisms"),
    ("TRAINS", "BRAKE"): ("Machinery", "Hydraulics & Pneumatics"),

    # USER INTERFACE: the branch is our UI tree, one entry at a time.
    ("USER INTERFACE", "ALERT"): ("UI & Game", "Notifications & Alerts"),
    ("USER INTERFACE", "BEEP"): ("UI & Game", "Notifications & Alerts"),
    ("USER INTERFACE", "CLICK"): ("UI & Game", "Clicks & Buttons"),
    ("USER INTERFACE", "DATA"): ("Sci-Fi & Tech", "Computers & Data"),
    ("USER INTERFACE", "GLITCH"): ("Transitions", "Glitches & Stutters"),
    ("USER INTERFACE", "MOTION"): ("UI & Game", "Menus & Navigation"),

    # VEHICLES: the two-wheelers and rail split off.
    ("VEHICLES", "MOTORCYCLE"): ("Vehicles", "Motorcycles & Bikes"),
    ("VEHICLES", "BICYCLE"): ("Vehicles", "Motorcycles & Bikes"),
    ("VEHICLES", "ATV"): ("Vehicles", "Motorcycles & Bikes"),
    ("VEHICLES", "TRAIN"): ("Vehicles", "Rail"),
    ("VEHICLES", "BOAT"): ("Vehicles", "Watercraft"),
    ("VEHICLES", "HORN"): ("Vehicles", "Cars & Trucks"),
    ("VEHICLES", "DOOR"): ("Foley", "Doors & Locks"),
    ("VEHICLES", "BRAKE"): ("Machinery", "Hydraulics & Pneumatics"),
    ("VEHICLES", "SUSPENSION"): ("Machinery", "Hydraulics & Pneumatics"),
    ("VEHICLES", "MECHANISM"): ("Machinery", "Switches & Mechanisms"),
    ("VEHICLES", "TIRE"): ("Vehicles", "Cars & Trucks"),

    # VOICES: sung and futzed are their own branches.
    ("VOICES", "SINGING"): ("Vocals", "Choirs & Chants"),
    ("VOICES", "FUTZED"): ("Vocals", "Spoken & Dialogue"),
    ("VOICES", "HISTORICAL"): ("Vocals", "Spoken & Dialogue"),
    ("VOICES", "ALIEN"): ("Creatures", "Aliens & Robots"),

    # WATER: surf and underwater are their own classes.
    ("WATER", "SURF"): ("Nature", "Water & Rain"),
    ("WATER", "WAVE"): ("Nature", "Water & Rain"),
    ("WATER", "WATERFALL"): ("Nature", "Water & Rain"),
    ("WATER", "UNDERWATER"): ("Ambience", "Underwater"),
    ("WATER", "STEAM"): ("Machinery", "Hydraulics & Pneumatics"),
    ("WATER", "PLUMBING"): ("Foley", "Household & Office"),
    ("WATER", "DRAIN"): ("Foley", "Household & Office"),

    # WEATHER: hail on a surface is a material impact.
    ("WEATHER", "HAIL"): ("Impacts", "Crash & Collapse"),
    ("WEATHER", "STORM"): ("Nature", "Thunder & Storms"),

    # WHISTLES: the human reading is a vocalization.
    ("WHISTLES", "HUMAN"): ("Vocals", "Game Characters"),
    ("WHISTLES", "MECHANICAL"): ("Machinery", "Clocks & Ticking"),

    # WINDOWS: the covering is cloth, the knock is a door knock.
    ("WINDOWS", "COVERING"): ("Foley", "Cloth & Movement"),
    ("WINDOWS", "KNOCK"): ("Foley", "Doors & Locks"),

    # WINGS: insect and creature wings are creature movement.
    ("WINGS", "CREATURE"): ("Creatures", "Monsters & Beasts"),
    ("WINGS", "INSECT"): ("Nature", "Animals & Birds"),
}

# Synonyms cut even though their UCS entry has a home here. Every one was
# checked against the 280,805-path reference mirror: the counts in the comment
# are files / packs containing the word. The rule is that a word has to name the
# class, not merely appear near it.
DROP_SYNONYMS = {
    # English function words and file-naming boilerplate. They occur in tens of
    # thousands of paths across dozens of packs and name nothing.
    "one": "55142 files / 97 packs",
    "shots": "43723 / 100",
    "shot": "33856 / 74",
    "of": "26283 / 41",
    "and": "17003 / 57",
    "the": "generic article",
    "a": "generic article",
    "an": "generic article",
    "any": "generic determiner",
    "all": "generic determiner",
    "misc": "UCS's own 'everything else' bucket",
    "miscellaneous": "UCS's own 'everything else' bucket",
    "source": "UCS's own provenance bucket, not a sound class",
    "marke": "truncated entry in the UCS list",
    # Words whose UCS meaning is a process or a production stage rather than a
    # sound. They would fire on every designed pack.
    "morph": "36008 / 12 — a production technique, and the SMorph pack prefix",
    "designed": "a production technique, and a stop word already",
    "recorded": "provenance, not a class",
    "recordings": "provenance, not a class",
    "field": "both a recording method and a plain noun",
    "recording": "provenance, not a class",
    "studio": "provenance, not a class",
    "sound": "already a stop word",
    "sounds": "already a stop word",
    "audio": "already a stop word",
    "fx": "generic",
    "effect": "generic",
    "effects": "generic",
    "texture": "generic",
    "textures": "generic",
    "element": "generic",
    "elements": "generic",
    "layer": "generic",
    "layers": "generic",
    "sequence": "generic",
    "sequences": "generic",
    "variation": "generic",
    "variations": "generic",
    "version": "already a stop word",
    "multiple": "generic",
    "several": "generic",
    "various": "generic",
    "small": "size adjective; every class has small ones",
    "large": "size adjective",
    "big": "size adjective",
    "long": "shape adjective",
    "short": "shape adjective",
    "high": "direction and pitch both",
    "low": "direction and pitch both",
    "fast": "speed adjective",
    "slow": "speed adjective",
    "soft": "amplitude adjective",
    "hard": "appears in hard drive, hard disk, hard wood",
    "dry": "both wetness and dry recordings",
    "wet": "both wetness and wet reverb",
    "clean": "both cleanliness and clean recordings",
    "dirty": "both dirtiness and dirty recordings",
    # Nouns so general that they appear in every pack's naming grammar.
    "house": "17394 / 41 — house music as often as a building",
    "garage": "15992 / 30 — garage music as often as a building",
    "room": "every room tone, every interior, every reverb",
    "space": "both a room and outer space",
    "object": "generic",
    "objects": "generic",
    "thing": "generic",
    "things": "generic",
    "part": "already a stop word",
    "parts": "already a stop word",
    "piece": "generic",
    "item": "generic",
    "items": "generic",
    "group": "generic",
    "loop": "already a stop word",
    "loops": "already a stop word",
    "hit": "25 UCS entries across nine of our categories",
    "hits": "25 UCS entries across nine of our categories",
    "impact": "20 UCS entries across nine of our categories; the curated word stays",
    "impacts": "20 UCS entries across nine of our categories",
    "clap": "13 UCS entries across five of our categories; the curated word stays",
    "claps": "13 UCS entries across five of our categories",
    "beat": "music genre and cardiac",
    "beats": "music genre and cardiac",
    "kick": "two UCS branches, and the curated word already covers it",
    "clang": "both metal and ceramic and glass",
    "pop": "genre, UI pop, and a light impact",
    "boom": "16 UCS entries across four of our categories",
    "ding": "a bell, a notification, and a microwave",
    "buzz": "insect, electric, and UI",
    "ring": "telephone, bell, and jewellery",
    "charge": "electricity and a weapon charge",
    "charge up": "electricity and a weapon charge",
    "discharge": "electricity and a weapon",
}

# Phrases that are a substring of another phrase. Matching is `contains` on the
# normalised path, so `hi tec` fires inside `hi tech` and both targets score; the
# winner then depends on nothing but insertion order. These come from UCS's own
# list, where synonyms were normalised inconsistently.
DROP_PHRASES = {
    "hi tec": "substring of `hi tech` (Vehicles-Rail), and a truncation typo",
    # `one shot` is a file-structure word, not a sound. In UCS it is a synonym of
    # MUSICAL-PERFORMANCE; in this library it appears in 43,723 filenames across
    # 100 packs, so it would file the whole corpus as music.
    "one shot": "43,723 files / 100 packs — a file-structure word, not a sound",
    "avant garde": "a genre label; 3 UCS entries point at it and it names no sound",
}
