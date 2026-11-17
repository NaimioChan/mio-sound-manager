"""ReSlip's category taxonomy: the source of truth for dictionary.rs.

Edit this file, then run `python tools/gen-category-dict.py` (it writes
`crates/reslip-index/src/classify/dictionary.rs`) and `cargo test -p
reslip-index`.

How the vocabulary was chosen: measured against a real sample library (a
552k-path SoundLibrary, mirrored to a 271k subset for fast iteration). Every word
here was checked for how often it fires and how many packs it spans, so a word
like `epic` or `cinematic` — present in dozens of pack titles and therefore in
almost every path — is a stop word instead of a keyword that swallows a category.

Shape: {category: {subcategory: (keywords, cjk_hints, phrases)}}

`__bare__` is the category-level entry: words that name the category itself
rather than one of its subcategories. A subcategory hit outranks a bare-category
hit on a tie.

Chinese hints are at least two characters, deliberately. Single characters have
no word boundary in Chinese compounds and match the wrong word in this corpus:
手 matches 手机 (phone), 电 matches 电影 (film), 门 matches 门类 (category),
风 matches 风格 (style). Every hint below is an attested term from the library's
own folder names.
"""

TAXONOMY = {
    "Drums": {
        "__bare__": ("""drum drums""", """鼓组 打击鼓组""", """drum loop|top loop|break beat"""),
        "Kicks": ("""kick kicks bd bassdrum 808sub subkick thump drumkick""", """底鼓 大鼓 踢鼓""", ""),
        "Snares & Claps": ("""snare snares snr rimshot rimshots rim clap claps handsnap""", """军鼓 小鼓 拍手 掌声""", ""),
        "Hats & Cymbals": ("""hat hats hihat hihats hh cymbal cymbals ride rides crash crashes tambourine tambs""", """踩镲 镲片 吊镲 沙锤 铃鼓""", ""),
        "Percussion": ("""perc percussion percs conga congas bongo bongos djembe tabla tablas cajon darbuka timbale timbales woodblock cowbell agogo shaker shakers guiro cabasa claves maracas udu""", """打击乐 手鼓 木鱼 牛铃""", ""),
        "Toms": ("""tom toms floortom racktom rototom""", """通鼓 嗵鼓""", ""),
        "Fills & Rolls": ("""fill fills filler drumfill drumroll rudiment rudiments""", """加花""", """drum fill|snare roll|drum roll"""),
        "Breakbeats": ("""breakbeat breakbeats amenbreak thinkbreak""", """碎拍""", """amen break|funky drummer"""),
        "Drum Loops": ("""drumloop drumloops toploop toploops beatloop drumgroove""", """鼓循环""", """drum loop|top loop"""),
        "Drum Kits": ("""drumkit drumkits""", """套鼓""", """drum kit"""),
    },
    "Bass": {
        "Sub & 808": ("""sub subbass 808s bass808 sublayer sinebass""", """次低音 超低音""", """808 bass|sub bass"""),
        "Growls & Reeses": ("""reese wobble wobbles yoi screechbass""", """低音咆哮""", """bass growl"""),
        "Bass One Shots": ("""bass basses bassline basslines bassshot bassone""", """贝斯 电贝司 低音""", ""),
        "Bass Loops": ("""bassloop bassloops basstops""", """贝斯循环""", """bass loop|bass loops"""),
    },
    "Synths": {
        "Leads": ("""lead leads leadline supersaw squarelead pulseline""", """主音 主奏""", """synth lead"""),
        "Pads & Drones": ("""pad pads warmpad stringpad drone drones sustained texture textures soundscape soundscapes""", """音垫 铺底 持续音""", """synth pad"""),
        "Plucks & Arps": ("""pluck plucks arp arps arpeggio arpeggios arpeggiated""", """拨弦 琶音""", """synth pluck"""),
        "Stabs & Chords": ("""stab stabs chord chords chordsynth""", """和弦 短促和弦""", """synth stab"""),
        "Synth Loops": ("""synthloop synthloops melodicloop""", """合成循环""", """synth loop|synth loops"""),
        "Synth One Shots": ("""synth synths synthesizer synthesizers wavetable serum massive sylenth vital""", """合成器 合成""", ""),
    },
    "Keys & Mallets": {
        "Piano": ("""piano pianos grandpiano upright""", """钢琴""", ""),
        "Electric Piano & Organ": ("""rhodes wurlitzer wurli organ organs clav clavinet epiano keys""", """电钢 风琴 电钢琴""", ""),
        "Mallets & Bells": ("""mallet mallets marimba xylophone glockenspiel vibraphone vibes celesta kalimba chimes bell bells handpan hangdrum gong musicbox""", """马林巴 木琴 钟琴 风铃 八音盒""", ""),
    },
    "Guitars": {
        "Acoustic Guitar": ("""acousticguitar steelstring nylonstring""", """木吉他 原声吉他""", """acoustic guitar"""),
        "Electric Guitar": ("""guitar guitars gtr riff riffs strum palmute chug""", """吉他 电吉他""", """electric guitar"""),
        "Guitar Loops": ("""guitarloop guitarloops""", """吉他循环""", """guitar loop|guitar loops"""),
    },
    "Strings & Winds": {
        "Strings": ("""violin violins viola violas cello cellos contrabass doublebass pizzicato spiccato tremolo strings""", """弦乐 小提琴 中提琴 大提琴 二胡 古筝 琵琶 箜篌 马头琴 古琴""", """string section"""),
        "Brass": ("""trumpet trumpets trombone trombones tuba flugelhorn cornet frenchhorn brasssection""", """铜管 小号 长号 圆号 唢呐""", """brass section"""),
        "Woodwinds": ("""flute flutes clarinet clarinets oboe bassoon sax saxophone piccolo recorder""", """长笛 单簧管 双簧管 萨克斯 笛子""", ""),
        "World Instruments": ("""duduk erhu guqin pipa konghou zheng koto shakuhachi shamisen sitar oud bouzouki didgeridoo yangqin""", """陶笛 扬琴 三味线 排箫""", ""),
        "Harps": ("""harp harps harpsichord lyre""", """竖琴""", ""),
    },
    "Vocals": {
        "Vocal One Shots": ("""vocal vocals vox voice acapella acappella acapellas""", """人声 演唱""", """vocal shot"""),
        "Chops": ("""chop chops vocalchop choppedvox""", """人声切片""", """vocal chop"""),
        "Choirs & Chants": ("""choir choirs choral chant chants chanting mantra hymn""", """合唱 圣歌 吟唱""", ""),
        "Spoken & Dialogue": ("""spoken dialogue dialog narration narrator announcer whisper whispers""", """对白 旁白 语音""", """stock line|spoken word"""),
        "Rap & Adlibs": ("""rap rapping adlib adlibs freestyle""", """说唱""", ""),
        "Game Characters": ("""grunt grunts effort yell yells shout shouts scream screams battlecry warcry deathcry vocalization vocalizations emote emotes""", """角色配音 呐喊 惨叫 尖叫""", """character vocalization"""),
        "Vocal Loops": ("""vocalloop vocalloops""", """人声循环""", """vocal loop|vocal loops"""),
    },
    "Music": {
        "__bare__": ("""music beat beats""", """歌曲 曲目""", ""),
        "Full Tracks": ("""fulltrack song songs soundtrack cue""", """完整曲目 配乐""", ""),
        "Stems": ("""stem multitrack instrumental""", """分轨""", ""),
        "Musical Loops": ("""musicloop musicloops progression""", """配乐循环""", """music loop"""),
    },
    "Foley": {
        "__bare__": ("""foley""", """拟音""", ""),
        "Footsteps": ("""footstep footsteps step boot boots shoe shoes heel heels sneaker slipper""", """脚步 走路 靴子""", """foot steps"""),
        "Cloth & Movement": ("""cloth clothes fabric rustle rustles jacket sleeve zipper velcro apparel""", """衣服 布料 摩擦""", ""),
        "Body & Hands": ("""hand hands finger fingers fingerclick breath breathing sigh swallow""", """身体 拍打""", ""),
        "Doors & Locks": ("""door doors drawer drawers latch latches lock locks handle knob hinge gate cabinet""", """舱门 电子门 大门 门锁 抽屉 飞船舱门""", """door open|door close|door slam"""),
        "Kitchen & Food": ("""kitchen cook cooking food eating bite chew sip kettle microwave fridge oven cutlery dish plate chopping frying boiling""", """厨房 烹饪 吃饭 餐具""", ""),
        "Household & Office": ("""household office desk chair vacuum broom mop cleaning wash washing toothbrush shower toilet towel faucet flush""", """家居 办公 打扫""", ""),
        "Objects & Props": ("""prop props object objects pickup dropobject grab fumble toolbox toolbag""", """道具 物品 拿起 放下""", ""),
        "Paper & Packaging": ("""paper cardboard bag bags box boxes package parcel wrap wrapping crinkle crinkles""", """纸张 包装 塑料袋""", ""),
        "Sports & Toys": ("""ball bounce skid skateboard toy toys playground swing""", """玩具 皮球""", ""),
        "Water Foley": ("""splash splashes drip drips dropwater pour gurgle bubble bubbles slosh squish""", """水滴 水花 倒水 水声""", ""),
    },
    "Impacts": {
        "Metal": ("""metal metallic clang clangs clank iron steel aluminum aluminium anvil pipe pipes chain chains""", """金属 铁链 钢制""", ""),
        "Wood": ("""wood wooden timber plank planks trunk log clatter""", """木头 木质 木材""", ""),
        "Glass": ("""glass glasses shatter shatters smash crystal bottle bottles""", """玻璃 碎玻璃""", ""),
        "Plastic & Ceramic": ("""plastic ceramic porcelain clay pottery terracotta""", """塑料 陶瓷""", ""),
        "Stone & Concrete": ("""stone stones concrete cement brick bricks gravel rubble debris""", """石头 混凝土 砖块 碎屑""", ""),
        "Hard Hits": ("""hit hits impact impacts slam slams thud thuds whack bash knock pound""", """撞击 冲击 打击 碰撞 碎裂""", ""),
        "Punches & Body Blows": ("""punch punches slap smack fist jab uppercut blow blows""", """拳击 殴打 拳打脚踢 打斗""", """punch hit"""),
        "Crash & Collapse": ("""crumble collapse crashing wreck wreckage""", """崩塌 废墟 破坏 炸毁""", ""),
        "Sub Drops": ("""subdrop subdrops subbomb""", """低频坠落""", """sub drop"""),
    },
    "Weapons": {
        "__bare__": ("""weapon weapons weaponry""", """兵器""", ""),
        "Firearms": ("""gun guns firearm firearms gunshot gunshots shoot shooting rifle rifles carbine pistol pistols handgun handguns revolver revolvers shotgun shotguns smg submachinegun machinegun minigun sniper magnum deagle glock beretta uzi ak47 akm m16 m4 m4a1 m249 mp5 musket flintlock blunderbuss derringer""", """步枪 手枪 冲锋枪 机枪 狙击枪 霰弹枪 枪械 枪声""", """assault rifle|sniper rifle|machine gun"""),
        "Ammo & Handling": ("""bullet bullets shell shells casing casings brass cartridge cartridges ammo ammunition magazine magazines clip reload reloading racking bolt chamber chambering cock cocking trigger safety suppressor silencer muzzle recoil ricochet dryfire""", """子弹 弹壳 换弹 上膛 枪栓 弹匣 消音 枪械操作""", """shell casing|dry fire|muzzle flash"""),
        "Melee & Blades": ("""sword swords blade blades katana katanas sabre saber rapier longsword broadsword cutlass scimitar dagger daggers knife knives machete axe axes hatchet hammer mace spear spears pike halberd staff baton club flail sheath sheathing sharpen whet""", """刀剑 匕首 长剑 长矛 武士刀 斧头 铁锤""", ""),
        "Bows & Arrows": ("""bow bows arrow arrows crossbow crossbows quiver drawbow""", """弓箭 弩箭""", """bow draw"""),
        "Explosives": ("""grenade grenades bomb bombs dynamite tnt mortar artillery mine landmine rocket rocketlauncher warhead missile missiles""", """手榴弹 炸弹 火箭弹""", """grenade launcher|rocket launcher"""),
        "Sci-Fi Weapons": ("""laser lasers blaster blasters plasma phaser phasers railgun ioncannon disintegrator pulser""", """激光枪 激光武器 科幻武器 等离子武器""", ""),
        "Casing Drops": ("""bulletdrop brassdrop casingdrop""", """弹壳落地""", """bullet drop|shell drop"""),
    },
    "Explosions & Fire": {
        "Blasts": ("""explosion explosions blast blasts detonation detonations boom booms eruption blastwave shockwave""", """爆炸 爆破 冲击波""", """explosion blast"""),
        "Fire & Burning": ("""fire flames flame burning burn ignite ignition sizzle sizzles crackle crackling ember embers torch campfire wildfire""", """燃烧 火焰 火元素""", ""),
        "Fuse & Debris": ("""fuse sparks shrapnel incoming whistlebomb""", """引信""", ""),
    },
    "Vehicles": {
        "__bare__": ("""vehicle vehicles""", """载具 交通工具""", ""),
        "Cars & Trucks": ("""car cars truck trucks engine engines motor motors exhaust tire tires tyre horn horns brake brakes transmission turbo clutch""", """汽车 引擎 车辆 轮胎""", """car engine"""),
        "Motorcycles & Bikes": ("""motorcycle motorcycles motorbike motorbikes scooter scooters moped bicycle bicycles bike bikes""", """摩托 自行车""", ""),
        "Aircraft": ("""plane planes aircraft airplane jet jets helicopter helicopters heli rotor propeller propellers turbine jetengine uav""", """飞机 直升机 螺旋桨""", ""),
        "Watercraft": ("""boat boats ship ships yacht sailboat submarine canoe kayak ferry motorboat""", """轮船 潜艇 帆船""", ""),
        "Rail": ("""train trains rail rails railway locomotive tram subway metro crossing""", """火车 列车 地铁""", ""),
        "Sci-Fi Craft": ("""spaceship spaceships starship hovercar speeder warpdrive thruster thrusters""", """飞船 悬浮车 飞船引擎""", ""),
    },
    "Machinery": {
        "__bare__": ("""mechanical machine machines machinery""", """机械 机器""", ""),
        "Motors & Servos": ("""servo servos actuator actuators spindle electricmotor motorized""", """伺服 电机 马达""", ""),
        "Gears & Ratchets": ("""gear gears cog cogs ratchet ratchets sprocket winch pulley crank flywheel""", """齿轮 棘轮 链条""", ""),
        "Hydraulics & Pneumatics": ("""hydraulic hydraulics pneumatic pneumatics piston pistons valve valves compressor steam vent airrelease""", """液压 气动 气压""", ""),
        "Tools & Power": ("""drill driller saw chainsaw grinder sander wrench hammering screwdriver jackhammer nailgun welder welding industrial""", """电钻 扳手 工具 电锯""", """power tool"""),
        "Switches & Mechanisms": ("""switch switches lever coil gadget device""", """开关 机械装置""", ""),
        "Clocks & Ticking": ("""clock clocks tick ticking tock metronome timer countdown clockwork pendulum""", """时钟 计时""", ""),
    },
    "Nature": {
        "__bare__": ("""nature""", """自然""", ""),
        "Wind": ("""wind winds breeze gust gusts gale""", """风声 风暴 风系 狂风""", ""),
        "Water & Rain": ("""water rain raining river stream creek brook waterfall ocean sea waves surf lake pond puddle underwater""", """水元素 流水 河流 海浪 雨水 雨声""", ""),
        "Thunder & Storms": ("""thunder thunders thunderstorm storm storms lightning hail blizzard hurricane tornado""", """雷声 雷鸣 闪电 暴雨 暴风""", ""),
        "Animals & Birds": ("""dog dogs cat cats horse horses cow cows sheep goat pig pigs wolf wolves fox bear deer bird birds crow crows owl owls seagull pigeon chicken rooster duck geese goose frog frogs cricket crickets bee bees mosquito snake snakes""", """动物 鸟叫 狼嚎 虫鸣""", ""),
        "Vegetation & Earth": ("""forest jungle tree trees leaves leaf grass bush bushes branch branches dirt soil mud sand earthquake""", """森林 树叶 土壤""", ""),
    },
    "Ambience": {
        "__bare__": ("""ambience ambiance ambient atmosphere atmospheres""", """环境音 氛围 环境声""", ""),
        "Nature Beds": ("""ambiencebed""", """野外 荒野 生态""", ""),
        "Urban & City": ("""city urban street streets traffic crowd crowds market plaza airport station platform siren sirens construction""", """城市 街道 人群 交通""", ""),
        "Interior & Room Tone": ("""interior roomtone hallway corridor lobby restaurant cafe church classroom hospital ventilation conditioner fluorescent""", """室内 房间 走廊 办公室 机房""", """room tone"""),
        "Industrial": ("""factory warehouse docksite harbor generator engineroom""", """工厂 工业 车间""", ""),
        "Underwater": ("""submerged subaquatic aquatic""", """水下 海底""", ""),
        "Sci-Fi & Space": ("""space cosmic cosmos galactic nebula orbital future cyber laboratory reactor""", """太空 宇宙 实验室""", ""),
    },
    "Creatures": {
        "Monsters & Beasts": ("""monster monsters creature creatures beast beasts zombie zombies undead ghoul goblin orc troll ogre werewolf wendigo yeti bigfoot""", """怪物 怪兽 僵尸 狼人 雪怪 丧尸 魔兽 兽人 巨魔 哥布林 史前""", """monster roar|creature growl|beast growl"""),
        "Dragons & Reptiles": ("""dragon dragons drake wyvern reptile reptiles lizard lizards serpent basilisk""", """巨龙 蜥蜴""", """dragon roar"""),
        "Aliens & Robots": ("""alien aliens extraterrestrial robot robots droid droids""", """外星 异形 机器人""", ""),
        "Humanoids & Spirits": ("""demon demons devil devils imp angel angels spirit spirits ghost ghosts wraith phantom apparition poltergeist""", """恶魔 幽灵 鬼魂 精灵 亡灵""", ""),
        "Gore & Violence": ("""gore bloody blood flesh bone bones dismember squelch splatter butcher carnage""", """血腥 内脏 血肉 流血""", ""),
    },
    "Magic": {
        "Spells & Casts": ("""magic magical spell spells cast casting conjure conjuring incantation enchant enchantment sorcery wizard sorcerer witch warlock rune runes mystic mystical arcane""", """魔法 法术 施法 咒语 巫术 符文 魔法师 巫师 奥术""", """spell cast|magic cast"""),
        "Enchantments & Buffs": ("""buff buffs aura blessing blessed holy divine heal healing luck charm shield summon summoning portal portals""", """祝福 增益 光环 神圣 传送门 治疗""", ""),
        "Elemental Magic": ("""elemental fireball fireballs frost freezing energy""", """元素 火球 冰霜 元素魔法 魔法能量""", ""),
        "Dark & Cursed": ("""curse cursed hex hexed plague blight doom occult ritual sacrifice""", """诅咒 黑暗 仪式""", ""),
        "Potions & Items": ("""potion potions elixir flask vial scroll scrolls wand wands talisman amulet""", """药水 水晶 卷轴""", ""),
    },
    "Sci-Fi & Tech": {
        "__bare__": ("""scifi sci cyberpunk""", """科幻 未来科技 高科技""", ""),
        "Energy & Plasma": ("""beam beams pulse pulses charging overload forcefield deflector""", """等离子 脉冲 护盾 能量""", ""),
        "Beams & Rays": ("""raygun taser zap zaps""", """射线 激光束""", ""),
        "Computers & Data": ("""computer computers data digital cybernetic hack hacking encryption download upload matrix terminal""", """电脑 数据 数字 黑客 计算机""", ""),
        "Teleport & Warp": ("""teleport teleportation warping jumpgate hyperspace stargate wormhole""", """传送 跃迁""", ""),
        "Holograms & Scanners": ("""hologram holograms holographic scanner scan scanning sonar radar beacon""", """全息 扫描仪 雷达""", ""),
        "Circuit & Electric": ("""circuit circuitry voltage current electric electrical electricity spark arc arcing buzz buzzing electrocute""", """电流 电路 电压 电气""", ""),
    },
    "UI & Game": {
        "__bare__": ("""ui interface hud""", """用户界面 交互界面""", ""),
        "Clicks & Buttons": ("""click clicks tap taps press button buttons cursor hover select toggle""", """按钮 点击 电子按钮""", ""),
        "Notifications & Alerts": ("""beep beeps blip blips notify notification notifications alert alerts alarm message tone ding""", """通知 提示音 报警""", ""),
        "Menus & Navigation": ("""menu menus navigate navigation swipe tab inventory map quest confirm cancel""", """菜单 导航 取消""", ""),
        "Rewards & Progress": ("""coin coins reward rewards achievement achievements unlock levelup victory win success star stars score loot treasure""", """奖励 金币 升级 胜利 得分 解锁""", ""),
        "Errors & Warnings": ("""error errors wrong fail failed failure deny denied reject warning warnings forbidden invalid critical""", """错误 失败 警告""", ""),
        "Typing & Keyboards": ("""typing type keystroke keystrokes mouseclick touchscreen gesture""", """打字""", ""),
        "Card & Board Games": ("""card cards deck shuffle dice boardgame chess casino slot""", """卡牌 纸牌 骰子 棋盘""", ""),
    },
    "Transitions": {
        "Risers & Build-ups": ("""riser risers rise rising uplift uplifter buildup sweep sweeps swell tension""", """上升 渐强""", """build up|riser"""),
        "Downlifters & Falls": ("""downlifter downlifters fall faller falls descent descend downsweep""", """下降 渐弱""", ""),
        "Whooshes & Sweeps": ("""whoosh whooshes swoosh swooshes swish passby flyby transition transitions""", """呼啸 掠过 过渡 转场""", ""),
        "Reverses & Tape Stops": ("""reverse reverses reversed backwards tape tapestop spinback backspin""", """倒放 磁带停止""", ""),
        "Glitches & Stutters": ("""glitch glitches glitched stutter stutters repeater buffer skip skips""", """故障 卡顿 干扰""", ""),
        "Cinematic Hits": ("""braam braams stinger stingers""", """预告片 电影感""", """cinematic hit|trailer hit"""),
    },
    "Horror": {
        "Stingers & Scares": ("""horror scary scare scares eerie dread creepy sinister unsettling disturbing""", """恐怖 惊悚 毛骨悚然 诡异""", """jump scare"""),
        "Tension & Drones": ("""suspense ominous foreboding pulsing darkdrone""", """紧张 悬疑 低鸣""", ""),
    },
}
