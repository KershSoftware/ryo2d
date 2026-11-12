# Skeletal animation research: Spine, DragonBones, Spriter, engine formats, free art

Research date: 2026-11-12. Sources were fetched with a summarising fetcher. Quotes marked "quoted" were returned verbatim by the fetcher, but the fetcher could not always give full sentences (125-character quote limit), so re-read the linked pages before quoting in public. Items marked **unverified** came only from search-result snippets or failed fetches (403).

---

## 1. Spine (Esoteric Software)

### 1a. May a third party write its own runtime that reads Spine JSON/binary exports?

**Short answer: not clearly settled by the texts. The licences do not forbid it. The only documented route with no licence question is a clean-room runtime. No official Esoteric statement on third-party runtimes was found.**

Relevant texts:

- Spine Runtimes License Agreement, last updated April 5, 2025 (https://en.esotericsoftware.com/spine-runtimes-license, also https://raw.githubusercontent.com/EsotericSoftware/spine-runtimes/master/LICENSE). Quoted: "Integration of the Spine Runtimes into software or otherwise creating derivative works of the Spine Runtimes is permitted" and "provided that each user of the Products must obtain their own Spine Editor license" and "redistribution of the Products in any form must include this license and copyright notice." The agreement does not address exported data files, third-party runtimes written from scratch, or example assets (fetcher reports "Not addressed" for all three).
- Spine Editor License Agreement (https://en.esotericsoftware.com/spine-editor-license). Section 2 (Spine Runtimes): the runtimes "load, manipulate, and render animation data exported by the Spine Editor." Section 2.1 (quoted): "You may integrate the Spine Runtimes into software or otherwise create derivative works of the Spine Runtimes" with conditions including "each Product adds significant and primary functionality to the Spine Runtimes." Section 2.4(a): "integrate the Spine Runtimes into software or otherwise create derivative works of the Spine Runtimes; or". Section 2.4 example (quoted): "Each user of such an SDK, game toolkit, or software library must obtain a Spine Editor license". Section 12 (Restrictions) prohibits attempts to "reverse engineer, disassemble, decompile, decode" the Editor, except where law forbids that restriction. Note: that clause concerns the Editor, not the public JSON format.
- Spine JSON format doc (http://esotericsoftware.com/spine-json-format). Describes the JSON export in full, so a clean-room reader can be written from it.
- Spine Runtimes README (https://github.com/EsotericSoftware/spine-runtimes). Quoted: "You can integrate the Spine Runtimes into your software free of charge, but users of your software must have their own" (Spine licence) and "Please make your users aware of this requirement!"

**Unclear or conflicting points:**

- Forum staff replies on whether a distributor needs a Spine licence disagree in the search summaries. One reply says the party distributing the product needs a licence; another says unmodified distribution by a third party is fine without one. Forum posts are not authoritative, and the agreement text governs.
- A search snippet (not fetched) reports a forum thread where a developer building a Rust runtime from the public JSON docs under MIT was told they would need a clean-room implementation that does not read Esoteric's code. **Unverified:** the thread was not fetched, and the same snippet said this was staff guidance, not a legal opinion. The thread at https://en.esotericsoftware.com/forum/d/5274-question-about-runtime-licenses failed to load (site error) when fetched.
- A search summary says Esoteric recommends contacting contact@esotericsoftware.com for specific cases. **Unverified** beyond the summary.
- The runtime repo's own status: one search summary says Esoteric called the runtimes "open source under the Simplified BSD License." The fetched agreement text contains no such statement, and the fetched README does not use the phrase "open source." Treat "Simplified BSD" as unconfirmed and conflicting.

**Consequence for ryo2d:** a runtime written clean-room from the public JSON doc, with no Esoteric code and no spine-c/spine-cpp code, is not a derivative of the Spine Runtimes, so the Runtimes licence should not attach to it. That reasoning is my reading, not an Esoteric ruling. Get a written answer from Esoteric before relying on it.

### 1b. End users and a Spine licence

- Runtime users: quoted above. Yes, users of software containing the official runtimes need their own Spine Editor licence (Runtimes README and the agreement's "provided that each user..." wording).
- Exported data files alone: **not addressed** in the Editor or Runtimes licence text. The Runtimes README and Spine Editor licence are silent on whether a game shipping only exported JSON/binary files, with no runtime, needs a licence.
- Trial licence: the Editor licence Section 1.4.1 (quoted) says "The Spine Trial license does not grant rights to integrate, distribute, or otherwise make use of the Spine Runtimes."

### 1c. Current Spine JSON format version

- The JSON doc (http://esotericsoftware.com/spine-json-format) gives the example `"spine": "3.8.24"` in its skeleton object. The attribute list describes the field as "The version of Spine that exported the data. This can be used by tools to enforce a particular Spine version to be used." The attribute list calls the key "version" while the example uses "spine". The page has no format-change notes and does not say which Spine version it describes. **Conflicting and stale: the example is from the 3.8 era.**
- The current editor is Spine 4.3. The Spine changelog (https://en.esotericsoftware.com/spine-changelog) lists 4.3.27 as "Unreleased", 4.3.26 dated "7 Sep 2026" and 4.3.25 dated "6 Sep 2026". A search summary reports 4.3 stable launched 14 May 2026 (https://en.esotericsoftware.com/blog/Spine-4.3-released, not fetched).
- Answer: the JSON doc does not state a separate format version. The exporter version is the editor version (4.3.x now). Test against a 4.3.x export, and treat the 3.8 example as stale.

### 1d. May spineboy/raptor be redistributed in another project's demo?

**No licence found. Do not redistribute.**

- The example pages (https://en.esotericsoftware.com/spine-examples) contain no licence or usage text. The fetcher reported: "None." The footer reads "© 2013-2026 Esoteric Software LLC".
- The `examples/` folder of spine-runtimes lists only `readme.txt`, which has no licence statement (fetched via https://github.com/EsotericSoftware/spine-runtimes/tree/master/examples).
- The README says: "You are welcome to evaluate the Spine Runtimes and the examples we provide in this repository free of charge." This is evaluation wording, not a redistribution grant.
- A German forum question asking exactly whether Spineboy can be bundled in a free/OSS demo has no official reply in the search results (https://de.esotericsoftware.com/forum/d/2481, not fetched).
- Recommendation: use own art, or ask Esoteric in writing.

---

## 2. DragonBones

### 2a. Editor: maintained, downloadable, free?

**Unresolved. Official download and licence pages could not be fetched.**

- https://dragonbones.github.io/ returned only a title ("龙骨 - DragonBones 官方网站"). No download or licence text.
- The DragonBones GitHub org (https://github.com/DragonBones) shows the following last-update dates: DragonBonesJS Jan 23, 2026 (MIT); DragonBonesCSharp May 8, 2026 (MIT); DragonBonesUnity Jun 25, 2026 (licence not shown); DragonBonesCPP Jul 4, 2025 (MIT); Godot-DragonBones Mar 1, 2026 (MIT, a community GDExtension); Tools Jun 9, 2022 (MIT); DesignPanel Apr 20, 2021 (MIT). The repository list shows no DragonBones Pro/editor repo.
- Editor licence sources conflict:
  - AlternativeTo lists DragonBones as "Discontinued" and "Open Source (MIT)" (https://alternativeto.net/software/dragonbones, search summary only).
  - MajorGeeks lists DragonBones Pro 5.6.2 as "License: Open Source" (https://www.majorgeeks.com/files/details/dragonbones_pro.html, search summary only).
  - A Defold forum post says the runtimes are open source but the Pro editor is not, and parts need a web-service login (https://forum.defold.com/t/dragon-bones-4-9-5-supports-spine-3-3/3630, search summary only).
  - An itch.io devlog dated August 15, 2021 says only: "The free download for the Dragonbones library is now updated with the proper license terms." It quotes no licence text (https://itch.io/devlog/283893/updated-dragonbones-license-agreement.amp, fetched).
- Answer: the editor's current licence and download status are unknown. Do not assume it is free to redistribute or maintained. Community evidence suggests it is discontinued.

### 2b. Licence of JSON export format and runtimes

- Runtimes: MIT for the official GitHub repos where the label is shown (DragonBonesJS, DragonBonesCPP, DragonBonesCSharp, Tools). DragonBonesUnity licence not shown.
- JSON export format: no licence text found. The format doc is a community document in the dragonbones-tools npm package (https://unpkg.com/dragonbones-tools@0.1.2/doc/dragonbones_json_format_5.5.md). It does not name an author or claim to be an official spec.

### 2c. Features supported by the format (5.5 community doc, fetched)

- Meshes: yes, with `vertices`, `uvs`, `triangles`, `weights`, `slotPose`, `bonePose`, and shared meshes via `share` and `inheritDeform`.
- Weights: yes (on mesh display data).
- IK: yes. An `ik` list (bone, target, `bendPositive`, `chain`, `weight`) and an IK timeline.
- Skins: yes. A `skin` list maps slots to displays.
- Events: yes, frame events and frame sound events with `ints`, `floats`, `strings`.
- Deform: yes, called "FFD" in the doc (per-mesh vertex offset keyframes).
- Animation timelines: bone translate/rotate/scale, slot display and colour, z-order.
- Version in the file: `"version": "5.5"`.

### 2d. Official format spec?

- **No official spec found.** The 5.5 doc is community-maintained. A Defold forum post cites an older spec (DBSkeletonDataFormatSpec_V2.4_en.xml) and a 4.5 doc in the DragonBonesJS repo, not fetched. The DragonBones `Tools` README (https://github.com/DragonBones/Tools, fetched) names JSON format "V 5.5" and the converters `2db` and `db2`, with licence label "MIT license" and no licence text.

### 2e. Spriter (Brashmonkey, SCML)

- Editor: Spriter Pro is sold commercially. Steam listing shows $59.99 (date unclear, search summary). Official site https://brashmonkey.com/ links to a Spriter EULA (https://brashmonkey.com/?page_id=241), which the fetcher could not read. No licence text obtained.
- SCML format: no official spec found. Brashmonkey's site has no SCML spec text.
- Runtimes: community implementations only, licences mixed or unknown: SpriterHaxeEngine (MIT per search summary), SpriterAS (MIT per summary), Defold SCML parser (MIT per summary), spriter.js (licence not shown), SpriterDotNet (no licence shown), gdx-spriter (Zlib per summary). The Godot SCML importer shows MIT in one listing and AGPLv3 in another. Check each repo's LICENSE file before use.
- Verdict: not a realistic primary import target (commercial editor, no official spec, unclear licences).

### 2f. Godot and Unity 2D skeletal formats

- Godot: no native Spine/DragonBones/Spriter import. Skeleton2D is described as "The parent of a hierarchy of Bone2Ds, used to create a 2D skeletal animation." and holds "a reference to each Bone2D's rest pose." The page does not mention importing any external format (https://docs.godotengine.org/en/stable/classes/class_skeleton2d.html, fetched). Official Spine runtime for Godot 4 (spine-godot) is built on spine-cpp, not MIT, and needs a custom Godot build according to a search summary, which I did not verify; that claim conflicts with Godot 4 GDExtension examples, so treat it as unclear. Community DragonBones GDExtension Godot-DragonBones (MIT per GitHub org listing) supports Godot 4.2+ and DragonBones Pro 5.6 (search summary).
- Unity: the 2D Animation package docs (http://docs.unity3d.com/Packages/com.unity.2d.animation@18.0/index.html) fetched but gave only a home-page line. **Unverified:** whether the package imports any external skeletal format. Do not assume it does. Spine and DragonBones have Unity runtimes from their own organisations.

---

## 3. Free cutout-character art

| Candidate | Licence (verified?) | Download | Parts | Verdict |
|---|---|---|---|---|
| rgsdev, "Free CC0 Modular Animated Vector Characters 2D" | Verified: "The license is CC0, so you can use any way you want, even commercially." Asset field: "Creative Commons Zero v1.0 Universal." "Credits is not needed." (https://rgsdev.itch.io/free-cc0-modular-animated-vector-characters-2d, fetched) | Download is via https://rgsdev.itch.io/free-cc0-modular-animated-vector-characters-2d/purchase ("Name your own price"). File: "Free 2D Animated Vector Game Character Sprites.zip" (69 MB) | Animations: idle, walk, roll, jump, hit, death. 3 heads, 3 hairs, 7 eyes, 5 horns, 8 mouths, 1 body, 1 hand, 1 foot, 1 wing, 3 weapons, 8 pre-made characters. Parts are white for tinting. Canvas 2048x2048. Inkscape source only via Patreon. | **Top pick, licence verified.** Limb set is thin (one hand and one foot), so it is a starter rig at best. Check whether hand/foot are mirrored. |
| Kenney, "Modular Characters" (2014) | Verified: "Creative Commons CC0" (https://kenney.nl/assets/modular-characters, fetched). 425 files, 2D, v1.0. | https://kenney.nl/assets/modular-characters (download via on-page anchor; all-in-one bundle at https://kenney.itch.io/kenney-game-assets) | **Not verified.** Tags "character" and "modular". The page does not describe the parts. | **Top pick #2, licence verified, parts unverified.** Download and open the zip before committing. |
| Kenney, "Platformer Characters 1 (5 characters)" on OpenGameArt | **Unverified.** Search snippet says CC0 and "separated limbs (for software like Spine and Spriter) plus vector source files". OGA pages returned 403 to the fetcher (https://opengameart.org/content/platformer-characters-1-5-characters, https://opengameart.org/node/69755). | OGA page | Claimed separated limbs; not verified | Strong candidate if the OGA page confirms CC0 and limbs. |
| Kenney, Toon Characters 1 | CC0 verified (https://www.kenney.nl/assets/toon-characters-1) | kenney.nl | Not stated as parts; tagged "poses" | Not a cutout rig unless parts are checked. |
| PSRC Human Mk2 and Ultimate Devkit (Reactorcore) | **Not usable.** Itch page: "The contents of this pack have no licence and I don't care for licences to begin with" (https://reactorcore.itch.io/psrc-human-mk2-and-ultimate-devkit, fetched). Also includes adult anatomy. | itch / OGA node 179623 (403) | Torsos, limbs, hands, feet, heads, skeleton Mk2 | Reject: no licence. The earlier search snippet calling it CC0 was wrong. |
| Kenney, Creature Mixer | Custom licence (personal, educational, commercial with exclusions) | kenney.itch.io/creature-mixer | Separate sprites only as a paid add-on, $10 minimum | Reject: not CC0, paid parts. |
| Cabbit bases collection (OGA node 79237) | CC0 per search snippet only | OGA | Body, head, eyes, ears | **Unverified**, page not fetched. |
| Rubberduck, Customizable Character Pack (OGA) | CC0 per search snippet only (page 403) | OGA | Base, shoes, pants, tops, hair, GIMP file | **Unverified**. |

---

## Recommendation: import target

Options:

1. **Spine JSON (4.3.x export)**. Best documentation and tool support. Licence risk: third-party runtime reading exported files is not clearly settled. A clean-room reader from the public JSON doc, with no Esoteric code, is the most defensible route. Get Esoteric's written answer first.
2. **DragonBones 5.5 JSON**. Covers meshes, weights, IK, skins, events and deform. Format doc and converters are MIT-licensed or community-published, with no Esoteric-style restriction. Editor status is unclear and the format has no official spec.
3. **Spriter SCML**. Not recommended. Commercial editor, no official spec, mixed runtime licences.
4. **Godot/Unity native formats**. None exists for these purposes. Not an import target.

**Recommendation:** design a ryo2d native skeleton format first. Build the first importer for **Spine JSON** written clean-room from the public JSON doc, and only after a written OK from Esoteric. If that is refused or slow, use **DragonBones 5.5 JSON** as the fallback import target. Do not import Spineboy or Raptor assets into the repository.

---

## Gaps and things not verified

- DragonBones official site and download page not fetched successfully.
- Spriter EULA and SCML format not readable.
- Spine forum thread on runtime licences (https://en.esotericsoftware.com/forum/d/5274-question-about-runtime-licenses) did not load.
- OpenGameArt pages returned 403 to the fetcher, so OGA licences rely on search snippets.
- Unity 2D Animation package import capabilities not verified.
