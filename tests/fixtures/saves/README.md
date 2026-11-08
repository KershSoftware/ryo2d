# Save and settings fixtures

Files written in the formats that came before today's, kept so that every
later engine is tested against real player data from the older code. Never
regenerate them: their point is that they come from that code.

`legacy.ryo-save` and `system-format2.json` were written by the engine at
commit `56ed4806`, with `SaveManager` from `src/core/config/save.rs`, by a
throwaway test that is not kept. `system-format1.json` is hand-written.

## `legacy.ryo-save`

A game save in format 1: the magic `RYO2`, the version 1 (u32 little-endian),
then the deflated RON of `GameSaveData<LegacyFixture>`. Written with
`SaveManager::save_game_with_info`:

```rust
#[derive(Serialize, Deserialize)]
struct LegacyFixture {
  level: u32,
  name: String,
  items: Vec<String>,
}

let mut save = GameSaveData::new(&LegacyFixture {
  level: 3,
  name: "Ada".into(),
  items: vec!["key".into(), "lamp".into()],
})
.with_name("Before the update")
.with_play_time(754)
.with_metadata("chapter", "2");
save.timestamp = 1_700_000_000;
manager.save_game_with_info("legacy", save)?;
```

Its body inflates to:

```text
(data:(level:3,name:"Ada",items:["key","lamp"]),timestamp:1700000000,name:Some("Before the update"),play_time:754,metadata:{"chapter":"2"})
```

## `system-format2.json`

The settings file at format 2, as `SaveManager::save_system` writes it with no
file there: every key it knows. From `SystemState::new()` (the engine's
defaults) with `master_volume` 0.8 and `env` `{"difficulty": "hard"}`.

## `system-format1.json`

The settings file at format 1, from before `format_version`: its volumes are
amplitudes, heard at `20·log10(v)` dB. It holds `master_volume` 0.25,
`music_volume` 1.0, `resolution` `"1600x900"`, `display_mode` `"windowed"`,
`vsync` true and `env` `{"difficulty": "hard"}`.
