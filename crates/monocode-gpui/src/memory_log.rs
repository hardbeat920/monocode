//! A memory readout for profiling: launched with `MONOCODE_MEMORY_LOG=<file>`,
//! the app appends its footprint and GPU usage to that file every 10 seconds.
//! `open --env MONOCODE_MEMORY_LOG=/tmp/monocode-memory.log "Monocode Native.app"`
use gpui::App;
use std::{io::Write, time::Duration};

const PERIOD: Duration = Duration::from_secs(10);

pub(crate) fn start(cx: &mut App) {
    let Some(path) = std::env::var_os("MONOCODE_MEMORY_LOG") else {
        return;
    };
    let executor = cx.background_executor().clone();
    cx.background_executor()
        .spawn(async move {
            loop {
                let line = sample();
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                {
                    let _ = writeln!(file, "{line}");
                }
                executor.timer(PERIOD).await;
            }
        })
        .detach();
}

fn sample() -> String {
    let gpu = gpui_apple::metal_renderer::gpu_memory();
    let mb = |bytes: u64| bytes as f64 / (1024. * 1024.);
    format!(
        "{} footprint={:.1}MB gpu={:.1}MB instance_buffer={:.1}MB effect_textures={:.1}MB \
         windows={} drawables={:.1}MP atlas_mono={:.1}MB atlas_poly={:.1}MB atlas_textures={} \
         glyphs={:.1}MB svgs={:.1}MB images={:.1}MB trims={}",
        chrono::Local::now().format("%H:%M:%S"),
        mb(footprint()),
        mb(gpu.allocated),
        mb(gpu.instance_buffer),
        mb(gpu.scratch),
        gpu.renderers,
        gpu.drawable_pixels as f64 / 1e6,
        mb(gpu.atlas.monochrome),
        mb(gpu.atlas.polychrome),
        gpu.atlas.textures,
        mb(gpu.atlas.glyph_tiles),
        mb(gpu.atlas.svg_tiles),
        mb(gpu.atlas.image_tiles),
        gpu.trims,
    )
}

/// What Activity Monitor shows as Memory.
fn footprint() -> u64 {
    let mut info = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
    // SAFETY: the buffer is a rusage_info_v2, the flavor this call fills.
    let status = unsafe {
        libc::proc_pid_rusage(
            std::process::id() as i32,
            libc::RUSAGE_INFO_V2,
            info.as_mut_ptr().cast(),
        )
    };
    if status == 0 {
        // SAFETY: a successful call initialized the whole struct.
        unsafe { info.assume_init() }.ri_phys_footprint
    } else {
        0
    }
}
