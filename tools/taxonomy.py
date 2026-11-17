"""ReSlip's category taxonomy: the source of truth for dictionary.rs.

Edit this file, then run `python tools/gen-category-dict.py` (it writes
`crates/reslip-index/src/classify/dictionary.rs`) and `cargo test -p
reslip-index`.

How the vocabulary was chosen: measured against a real sample library (a
552k-path SoundLibrary, mirrored to a 271k subset for fast iteration). Every word
here was checked for how often it fires and how many packs it spans, so a word
like `epic` or `cinematic` — present in dozens of pack titles and therefore in
almost every path — is a stop word instead of a keyword that swallows a category.

Shape: {category: {subcategory: (keywords, phrases)}}

`__bare__` is the category-level entry: words that name the category itself
rather than one of its subcategories. A subcategory hit outranks a bare-category
hit on a tie.

Matching is ASCII only. Chinese folder and file names are not read at all: the
Epic Stock Media packs name their folders with a whole sentence about the pack's
theme, so any Chinese signal labels thousands of unrelated files at once.
"""

TAXONOMY = {
    "Drums": {
        "__bare__": ("""drum drums""", """drum loop|top loop|break beat"""),
        "Kicks": ("""kick kicks bd bassdrum 808sub subkick thump drumkick""", """"""),
        "Snares & Claps": ("""snare snares snr rimshot rimshots rim clap claps handsnap""", """"""),
        "Hats & Cymbals": ("""hat hats hihat hihats hh cymbal cymbals ride rides crash crashes tambourine tambs""", """"""),
        "Percussion": ("""perc percussion percussive percs conga congas bongo bongos djembe tabla tablas cajon darbuka timbale timbales woodblock cowbell agogo shaker shakers guiro cabasa claves maracas udu""", """"""),
        "Toms": ("""tom toms floortom racktom rototom""", """"""),
        "Fills & Rolls": ("""fill fills filler drumfill drumroll rudiment rudiments""", """drum fill|snare roll|drum roll"""),
        "Breakbeats": ("""break breakbeat breakbeats amenbreak thinkbreak""", """amen break|funky drummer|drum break"""),
        "Drum Loops": ("""drumloop drumloops toploop toploops beatloop drumgroove""", """drum loop|top loop"""),
        "Drum Kits": ("""drumkit drumkits""", """drum kit"""),
    },
    "Bass": {
        "Sub & 808": ("""sub subbass 808s bass808 sublayer sinebass""", """808 bass|sub bass"""),
        "Growls & Reeses": ("""reese wobble wobbles yoi screechbass""", """bass growl"""),
        "Bass One Shots": ("""bass basses bassline basslines bassshot bassone""", """"""),
        "Bass Loops": ("""bassloop bassloops basstops""", """bass loop|bass loops"""),
    },
    "Synths": {
        "Leads": ("""lead leads leadline supersaw squarelead pulseline""", """synth lead"""),
        "Pads & Drones": ("""pad pads warmpad stringpad drone drones sustained texture textures soundscape soundscapes""", """synth pad"""),
        "Plucks & Arps": ("""pluck plucks arp arps arpeggio arpeggios arpeggiated""", """synth pluck"""),
        "Stabs & Chords": ("""stab stabs chord chords chordsynth""", """synth stab"""),
        "Synth Loops": ("""synthloop synthloops melodicloop""", """synth loop|synth loops"""),
        "Synth One Shots": ("""synth synths synthesizer synthesizers wavetable serum massive sylenth vital""", """"""),
    },
    "Keys & Mallets": {
        "Piano": ("""piano pianos grandpiano upright""", """"""),
        "Electric Piano & Organ": ("""rhodes wurlitzer wurli organ organs clav clavinet epiano keys""", """"""),
        "Mallets & Bells": ("""mallet mallets marimba xylophone glockenspiel vibraphone vibes celesta kalimba chimes bell bells handpan hangdrum gong musicbox""", """"""),
    },
    "Guitars": {
        "Acoustic Guitar": ("""acousticguitar steelstring nylonstring""", """acoustic guitar"""),
        "Electric Guitar": ("""guitar guitars gtr riff riffs strum palmute chug""", """electric guitar"""),
        "Guitar Loops": ("""guitarloop guitarloops""", """guitar loop|guitar loops"""),
    },
    "Strings & Winds": {
        "Strings": ("""violin violins viola violas cello cellos contrabass doublebass pizzicato spiccato tremolo strings""", """string section"""),
        "Brass": ("""trumpet trumpets trombone trombones tuba flugelhorn cornet frenchhorn brasssection""", """brass section"""),
        "Woodwinds": ("""flute flutes clarinet clarinets oboe bassoon sax saxophone piccolo recorder""", """"""),
        "World Instruments": ("""duduk erhu guqin pipa konghou zheng koto shakuhachi shamisen sitar oud bouzouki didgeridoo yangqin""", """"""),
        "Harps": ("""harp harps harpsichord lyre""", """"""),
    },
    "Vocals": {
        "Vocal One Shots": ("""vocal vocals vox voice acapella acappella acapellas""", """vocal shot"""),
        "Chops": ("""chop chops vocalchop choppedvox""", """vocal chop"""),
        "Choirs & Chants": ("""choir choirs choral chant chants chanting mantra hymn""", """"""),
        "Spoken & Dialogue": ("""spoken dialogue dialog narration narrator announcer whisper whispers""", """stock line|spoken word"""),
        "Rap & Adlibs": ("""rap rapping adlib adlibs freestyle""", """"""),
        "Game Characters": ("""grunt grunts effort yell yells shout shouts scream screams battlecry warcry deathcry vocalization vocalizations vocalizing emote emotes""", """character vocalization"""),
        "Vocal Loops": ("""vocalloop vocalloops""", """vocal loop|vocal loops"""),
    },
    "Music": {
        "__bare__": ("""music beat beats""", """"""),
        "Full Tracks": ("""fulltrack song songs soundtrack cue""", """"""),
        "Stems": ("""stem multitrack instrumental""", """"""),
        "Musical Loops": ("""musicloop musicloops progression melodyloop songstarter""", """music loop|melody loop"""),
    },
    "Foley": {
        "__bare__": ("""foley""", """"""),
        "Footsteps": ("""footstep footsteps step boot boots shoe shoes heel heels sneaker slipper""", """foot steps"""),
        "Cloth & Movement": ("""cloth clothes fabric rustle rustles jacket sleeve zipper velcro apparel""", """"""),
        "Body & Hands": ("""hand hands finger fingers fingerclick breath breathing sigh swallow""", """"""),
        "Doors & Locks": ("""door doors drawer drawers latch latches lock locks handle knob hinge gate cabinet""", """door open|door close|door slam"""),
        "Kitchen & Food": ("""kitchen cook cooking food eating bite chew sip kettle microwave fridge oven cutlery dish plate chopping frying boiling""", """"""),
        "Household & Office": ("""household office desk chair vacuum broom mop cleaning wash washing toothbrush shower toilet towel faucet flush""", """"""),
        "Objects & Props": ("""prop props object objects pickup dropobject grab fumble toolbox toolbag""", """"""),
        "Paper & Packaging": ("""paper cardboard bag bags box boxes package parcel wrap wrapping crinkle crinkles""", """"""),
        "Sports & Toys": ("""ball bounce skid skateboard toy toys playground swing""", """"""),
        "Water Foley": ("""splash splashes drip drips dropwater pour gurgle bubble bubbles slosh squish""", """"""),
    },
    "Impacts": {
        "Metal": ("""metal metallic clang clangs clank iron steel aluminum aluminium anvil pipe pipes chain chains""", """"""),
        "Wood": ("""wood wooden timber plank planks trunk log clatter""", """"""),
        "Glass": ("""glass glasses shatter shatters smash crystal bottle bottles""", """"""),
        "Plastic & Ceramic": ("""plastic ceramic porcelain clay pottery terracotta""", """"""),
        "Stone & Concrete": ("""stone stones concrete cement brick bricks gravel rubble debris""", """"""),
        "Hard Hits": ("""hit hits impact impacts slam slams thud thuds whack bash knock pound strike strikes""", """"""),
        "Punches & Body Blows": ("""punch punches slap smack fist jab uppercut blow blows""", """punch hit"""),
        "Crash & Collapse": ("""crumble collapse crashing wreck wreckage""", """"""),
        "Sub Drops": ("""subdrop subdrops subbomb""", """sub drop"""),
    },
    "Weapons": {
        "__bare__": ("""weapon weapons weaponry""", """"""),
        "Firearms": ("""gun guns firearm firearms gunshot gunshots shoot shooting rifle rifles carbine pistol pistols handgun handguns revolver revolvers shotgun shotguns smg submachinegun machinegun minigun sniper magnum deagle glock beretta uzi ak47 akm m16 m4 m4a1 m249 mp5 musket flintlock blunderbuss derringer""", """assault rifle|sniper rifle|machine gun"""),
        "Ammo & Handling": ("""bullet bullets shell shells casing casings brass cartridge cartridges ammo ammunition magazine magazines clip reload reloading racking bolt chamber chambering cock cocking trigger safety suppressor silencer muzzle recoil ricochet dryfire""", """shell casing|dry fire|muzzle flash"""),
        "Melee & Blades": ("""sword swords blade blades katana katanas sabre saber rapier longsword broadsword cutlass scimitar dagger daggers knife knives machete axe axes hatchet hammer mace spear spears pike halberd staff baton club flail sheath sheathing sharpen whet""", """"""),
        "Bows & Arrows": ("""bow bows arrow arrows crossbow crossbows quiver drawbow""", """bow draw"""),
        "Explosives": ("""grenade grenades bomb bombs dynamite tnt mortar artillery mine landmine rocket rocketlauncher warhead missile missiles""", """grenade launcher|rocket launcher"""),
        "Sci-Fi Weapons": ("""laser lasers blaster blasters plasma phaser phasers railgun ioncannon disintegrator pulser""", """"""),
        "Casing Drops": ("""bulletdrop brassdrop casingdrop""", """bullet drop|shell drop"""),
    },
    "Explosions & Fire": {
        "Blasts": ("""explosion explosions blast blasts detonation detonations boom booms eruption blastwave shockwave""", """explosion blast"""),
        "Fire & Burning": ("""fire flames flame burning burn ignite ignition sizzle sizzles crackle crackling ember embers torch campfire wildfire""", """"""),
        "Fuse & Debris": ("""fuse sparks shrapnel incoming whistlebomb""", """"""),
    },
    "Vehicles": {
        "__bare__": ("""vehicle vehicles""", """"""),
        "Cars & Trucks": ("""car cars truck trucks engine engines motor motors exhaust tire tires tyre horn horns brake brakes transmission turbo clutch""", """car engine"""),
        "Motorcycles & Bikes": ("""motorcycle motorcycles motorbike motorbikes scooter scooters moped bicycle bicycles bike bikes""", """"""),
        "Aircraft": ("""plane planes aircraft airplane jet jets helicopter helicopters heli rotor propeller propellers turbine jetengine uav""", """"""),
        "Watercraft": ("""boat boats ship ships yacht sailboat submarine canoe kayak ferry motorboat""", """"""),
        "Rail": ("""train trains rail rails railway locomotive tram subway metro crossing""", """"""),
        "Sci-Fi Craft": ("""spaceship spaceships starship hovercar speeder warpdrive thruster thrusters""", """"""),
    },
    "Machinery": {
        "__bare__": ("""mechanical machine machines machinery mech mechs""", """"""),
        "Motors & Servos": ("""servo servos actuator actuators spindle electricmotor motorized bot bots""", """"""),
        "Gears & Ratchets": ("""gear gears cog cogs ratchet ratchets sprocket winch pulley crank flywheel""", """"""),
        "Hydraulics & Pneumatics": ("""hydraulic hydraulics pneumatic pneumatics piston pistons valve valves compressor steam vent airrelease""", """"""),
        "Tools & Power": ("""drill driller saw chainsaw grinder sander wrench hammering screwdriver jackhammer nailgun welder welding industrial""", """power tool"""),
        "Switches & Mechanisms": ("""switch switches lever coil gadget device""", """"""),
        "Clocks & Ticking": ("""clock clocks tick ticking tock metronome timer countdown clockwork pendulum""", """"""),
    },
    "Nature": {
        "__bare__": ("""nature""", """"""),
        "Wind": ("""wind winds breeze gust gusts gale""", """"""),
        "Water & Rain": ("""water rain raining river stream creek brook waterfall ocean sea waves surf lake pond puddle underwater""", """"""),
        "Thunder & Storms": ("""thunder thunders thunderstorm storm storms lightning hail blizzard hurricane tornado""", """"""),
        "Animals & Birds": ("""dog dogs cat cats horse horses cow cows sheep goat pig pigs wolf wolves bear deer bird birds birdsong crow crows owl owls seagull pigeon chicken rooster duck geese goose frog frogs cricket crickets bee bees mosquito snake snakes animals animal""", """"""),
        "Vegetation & Earth": ("""forest jungle tree trees leaves leaf grass bush bushes branch branches dirt soil mud sand earthquake""", """"""),
    },
    "Ambience": {
        "__bare__": ("""ambience ambiance ambient atmosphere atmospheres""", """"""),
        "Nature Beds": ("""ambiencebed""", """"""),
        "Urban & City": ("""city urban street streets traffic crowd crowds market plaza airport station platform siren sirens construction""", """"""),
        "Interior & Room Tone": ("""interior roomtone hallway corridor lobby restaurant cafe church classroom hospital ventilation conditioner fluorescent""", """room tone"""),
        "Industrial": ("""factory warehouse docksite harbor generator engineroom""", """"""),
        "Underwater": ("""submerged subaquatic aquatic""", """"""),
        "Sci-Fi & Space": ("""space cosmic cosmos galactic nebula orbital future cyber laboratory reactor""", """"""),
    },
    "Creatures": {
        "Monsters & Beasts": ("""monster monsters creature creatures beast beasts zombie zombies undead ghoul goblin orc troll ogre werewolf wendigo yeti bigfoot""", """monster roar|creature growl|beast growl"""),
        "Dragons & Reptiles": ("""dragon dragons drake wyvern reptile reptiles lizard lizards serpent basilisk""", """dragon roar"""),
        "Aliens & Robots": ("""alien aliens extraterrestrial robot robots droid droids""", """"""),
        "Humanoids & Spirits": ("""demon demons devil devils imp angel angels spirit spirits ghost ghosts wraith phantom apparition poltergeist""", """"""),
        "Gore & Violence": ("""gore bloody blood flesh bone bones dismember squelch splatter butcher carnage""", """"""),
    },
    "Magic": {
        "Spells & Casts": ("""magic magical spell spells cast casting conjure conjuring incantation enchant enchantment sorcery wizard sorcerer witch warlock rune runes mystic mystical arcane""", """spell cast|magic cast"""),
        "Enchantments & Buffs": ("""buff buffs aura blessing blessed holy divine heal healing luck charm shield summon summoning portal portals""", """"""),
        "Elemental Magic": ("""elemental fireball fireballs frost freezing energy""", """"""),
        "Dark & Cursed": ("""curse cursed hex hexed plague blight doom occult ritual sacrifice""", """"""),
        "Potions & Items": ("""potion potions elixir flask vial scroll scrolls wand wands talisman amulet""", """"""),
    },
    "Sci-Fi & Tech": {
        "__bare__": ("""scifi sci cyberpunk""", """"""),
        "Energy & Plasma": ("""beam beams pulse pulses charging overload forcefield deflector""", """"""),
        "Beams & Rays": ("""raygun taser zap zaps""", """"""),
        "Computers & Data": ("""computer computers data digital cybernetic hack hacking encryption download upload matrix terminal""", """"""),
        "Teleport & Warp": ("""teleport teleportation warping jumpgate hyperspace stargate wormhole""", """"""),
        "Holograms & Scanners": ("""hologram holograms holographic scanner scan scanning sonar radar beacon""", """"""),
        "Circuit & Electric": ("""circuit circuitry voltage current electric electrical electricity spark arc arcing buzz buzzing electrocute""", """"""),
    },
    "UI & Game": {
        "__bare__": ("""ui interface hud""", """"""),
        "Clicks & Buttons": ("""click clicks tap taps press button buttons cursor hover select toggle""", """"""),
        "Notifications & Alerts": ("""beep beeps blip blips notify notification notifications alert alerts alarm message tone ding""", """"""),
        "Menus & Navigation": ("""menu menus navigate navigation swipe tab inventory map quest confirm cancel""", """"""),
        "Rewards & Progress": ("""coin coins reward rewards achievement achievements unlock levelup victory win success star stars score loot treasure""", """"""),
        "Errors & Warnings": ("""error errors wrong fail failed failure deny denied reject warning warnings forbidden invalid critical""", """"""),
        "Typing & Keyboards": ("""typing type keystroke keystrokes mouseclick touchscreen gesture""", """"""),
        "Card & Board Games": ("""card cards deck shuffle dice boardgame chess casino slot""", """"""),
    },
    "Transitions": {
        "Risers & Build-ups": ("""riser risers rise rising uplift uplifter upsweep upsweeps buildup sweep sweeps swell tension""", """build up|riser"""),
        "Downlifters & Falls": ("""downlifter downlifters fall faller falls descent descend downsweep""", """"""),
        "Whooshes & Sweeps": ("""whoosh whooshes swoosh swooshes swish passby flyby transition transitions""", """"""),
        "Reverses & Tape Stops": ("""reverse reverses reversed backwards tape tapestop spinback backspin""", """"""),
        "Glitches & Stutters": ("""glitch glitches glitched stutter stutters repeater buffer skip skips""", """"""),
        "Cinematic Hits": ("""braam braams stinger stingers""", """cinematic hit|trailer hit"""),
    },
    "Horror": {
        "Stingers & Scares": ("""horror scary scare scares eerie dread creepy sinister unsettling disturbing""", """jump scare"""),
        "Tension & Drones": ("""suspense ominous foreboding pulsing darkdrone""", """"""),
    },
}
