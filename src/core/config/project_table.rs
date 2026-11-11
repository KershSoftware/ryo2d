// The project settings: each Ryo.toml key that becomes an engine setting,
// declared once. An entry gives its `ProjectLayer` field and type, its
// section and key, the `RYO_EMBED_*` key `ryo2d build` ships it as (if any)
// and a sample unlike the default (in the build under test: the console and
// the dev window keys default on with the developer tools, off without), for
// the tests that check every entry.
// `workspace_keys` are the other keys of the known sections, which the
// workspace reads itself, and `env` is `[env]` shipped as JSON.
//
// The crate reads this file in `config/project.rs`, and `src/build.rs`
// includes it to track the embed keys.
project_settings! {
  settings {
    title: String = project.title, embed "RYO_EMBED_TITLE", sample "Sample Title";

    resolution: (u32, u32) = system.resolution, embed "RYO_EMBED_RESOLUTION", sample "1600x900";
    pixel_art: bool = system.pixel_art, embed "RYO_EMBED_PIXEL_ART", sample true;
    display_mode: DisplayMode = system.display_mode, embed "RYO_EMBED_DISPLAY_MODE", sample "borderless_fullscreen";
    vsync: bool = system.vsync, embed "RYO_EMBED_VSYNC", sample false;
    shadow_quality: ShadowQuality = system.shadow_quality, embed "RYO_EMBED_SHADOW_QUALITY", sample "ultra";
    master_volume: f32 = system.master_volume, embed "RYO_EMBED_MASTER_VOLUME", sample 0.3;
    music_volume: f32 = system.music_volume, embed "RYO_EMBED_MUSIC_VOLUME", sample 0.4;
    sfx_volume: f32 = system.sfx_volume, embed "RYO_EMBED_SFX_VOLUME", sample 0.5;
    ui_volume: f32 = system.ui_volume, embed "RYO_EMBED_UI_VOLUME", sample 0.6;
    voice_volume: f32 = system.voice_volume, embed "RYO_EMBED_VOICE_VOLUME", sample 0.7;
    ambience_volume: f32 = system.ambience_volume, embed "RYO_EMBED_AMBIENCE_VOLUME", sample 0.8;
    // 0: the device's own buffer
    audio_buffer_frames: u32 = system.audio_buffer_frames, embed "RYO_EMBED_AUDIO_BUFFER_FRAMES", sample 256;
    show_fps: bool = system.show_fps, embed "RYO_EMBED_SHOW_FPS", sample true;
    background_fps: u32 = system.target_background_fps, embed "RYO_EMBED_BACKGROUND_FPS", sample 15;
    background_rendering: bool = system.background_rendering, embed "RYO_EMBED_BACKGROUND_RENDERING", sample false;

    show_colliders: bool = debug.show_colliders, embed "RYO_EMBED_SHOW_COLLIDERS", sample true;
    show_velocities: bool = debug.show_velocities, embed "RYO_EMBED_SHOW_VELOCITIES", sample true;
    show_origins: bool = debug.show_origins, embed "RYO_EMBED_SHOW_ORIGINS", sample true;
    show_frame_timing: bool = debug.show_frame_timing, embed "RYO_EMBED_SHOW_FRAME_TIMING", sample true;
    disable_debug_keys: bool = debug.disable_debug_keys, embed "RYO_EMBED_DISABLE_DEBUG_KEYS", sample true;
    enable_profiler: bool = debug.enable_profiler, embed "RYO_EMBED_ENABLE_PROFILER", sample true;
    logger_window: bool = debug.logger_window, embed "RYO_EMBED_LOGGER_WINDOW", sample true;
    dev_window_keys: bool = debug.dev_window_keys, embed "RYO_EMBED_DEV_WINDOW_KEYS", sample !cfg!(dev_tools);
    dev_console: bool = debug.console, embed "RYO_EMBED_CONSOLE", sample !cfg!(dev_tools);

    // Not embedded: a release finds icon.png beside the game
    icon: PathBuf = release.logo, sample "art/logo.png";

    // An asset path; empty: `project.tiled-project` at the asset root
    tiled_project: String = tilemap.tiled_project, embed "RYO_EMBED_TILED_PROJECT", sample "maps/game.tiled-project";
  }

  workspace_keys {
    project.name,
    project.version,
    project.description,
    paths.entry_point,
    paths.save_data,
    paths.assets,
    release.portable embed "RYO_EMBED_PORTABLE",
    release.save_folder embed "RYO_EMBED_SAVE_FOLDER",
  }

  env embed "RYO_EMBED_ENV";
}
