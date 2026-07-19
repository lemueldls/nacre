use taffy::prelude::*;
use vello::peniko::Color;

use crate::{
    bar::{ModuleValue, ResolvedModule},
    config::BarConfig,
    graphics::GraphicsEngine,
};

fn format_system_info(
    time_str: &str,
    cpu_pct: f32,
    mem_pct: f32,
    battery_pct: Option<f32>,
) -> String {
    match battery_pct {
        Some(pct) => format!("CPU {cpu_pct:.0}%  MEM {mem_pct:.0}%  BAT {pct:.0}%  {time_str}"),
        None => format!("CPU {cpu_pct:.0}%  MEM {mem_pct:.0}%  {time_str}"),
    }
}

/// Real content width for a module, via `GraphicsEngine::measure_text_width`
/// rather than a character-count guess, so the taffy box this sizes
/// matches what `draw_module` actually draws into it.
fn module_content_width(
    graphics: &mut GraphicsEngine,
    module: &ResolvedModule,
    font_size: f32,
) -> f32 {
    match &module.value {
        ModuleValue::Workspaces(workspaces) => {
            let count = workspaces.len().max(1) as f32;
            count * workspace_dot_size(font_size)
                + (count - 1.0).max(0.0) * workspace_gap(font_size)
        }
        ModuleValue::Title(title) => {
            graphics.measure_text_width(title.as_deref().unwrap_or(""), font_size)
        }
        ModuleValue::SystemInfo {
            time_str,
            cpu_pct,
            mem_pct,
            battery_pct,
            ..
        } => {
            graphics.measure_text_width(
                &format_system_info(time_str, *cpu_pct, *mem_pct, *battery_pct),
                font_size,
            )
        }
        ModuleValue::Plugin { .. } => 0.0,
    }
}

struct LaidOutModule<'a> {
    module: &'a ResolvedModule,
    x: f32,
}

/// Lays out one alignment group (start/center/end) as a single-row flex
/// container and returns each module's x-offset *within that group*, plus
/// the group's total width. The three groups get positioned relative to
/// each other by `render_bar` (start-aligned, end-aligned, and centered on
/// the full bar width), since taffy's own `JustifyContent::SpaceBetween`
/// doesn't give a true center once the three groups have unequal widths.
fn layout_group<'m>(
    graphics: &mut GraphicsEngine,
    modules: &'m [ResolvedModule],
    bar_height: f32,
    font_size: f32,
) -> (Vec<LaidOutModule<'m>>, f32) {
    if modules.is_empty() {
        return (Vec::new(), 0.0);
    }

    let padding = module_padding(font_size);

    let mut tree: TaffyTree<()> = TaffyTree::new();
    let mut leaves = Vec::with_capacity(modules.len());

    for module in modules {
        let content_width = module_content_width(graphics, module, font_size);
        let style = Style {
            size: Size {
                width: length(content_width + padding * 2.0),
                height: length(bar_height),
            },
            ..Default::default()
        };
        leaves.push(tree.new_leaf(style).expect("bar layout leaf"));
    }

    let root = tree
        .new_with_children(
            Style {
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::CENTER),
                gap: Size {
                    width: length(module_gap(font_size)),
                    height: zero(),
                },
                ..Default::default()
            },
            &leaves,
        )
        .expect("bar layout root");

    tree.compute_layout(root, Size::MAX_CONTENT)
        .expect("bar layout compute");

    let total_width = tree.layout(root).expect("bar layout read").size.width;

    let laid_out = modules
        .iter()
        .zip(leaves.iter())
        .map(|(module, &leaf)| {
            LaidOutModule {
                module,
                x: tree
                    .layout(leaf)
                    .expect("bar module layout read")
                    .location
                    .x,
            }
        })
        .collect();

    (laid_out, total_width)
}

/// Draws every configured bar module for one frame. Call this after
/// `graphics.clear()` and before presenting; it only issues draw calls into
/// `graphics.scene`, it doesn't touch the wgpu surface or present a frame
/// itself.
///
/// `font_size` is `config.theme.font.size`; every spacing constant used
/// for layout is derived from it (see the module-level functions above)
/// rather than being independently hardcoded.
pub fn render_bar(
    graphics: &mut GraphicsEngine,
    bar_config: &BarConfig,
    modules: (
        Vec<ResolvedModule>,
        Vec<ResolvedModule>,
        Vec<ResolvedModule>,
    ),
    bar_width: f32,
    font_size: f32,
    accent_color: Color,
) {
    let bar_height = bar_config.height as f32;
    let padding = module_padding(font_size);
    let gap = module_gap(font_size);
    let (start, center, end) = modules;

    let (start_laid, start_width) = layout_group(graphics, &start, bar_height, font_size);
    let (center_laid, center_width) = layout_group(graphics, &center, bar_height, font_size);
    let (end_laid, end_width) = layout_group(graphics, &end, bar_height, font_size);

    let start_origin = padding;
    let center_origin = ((bar_width - center_width) / 2.0).max(start_origin + start_width + gap);
    let end_origin = (bar_width - padding - end_width).max(center_origin + center_width + gap);

    for (origin, group) in [
        (start_origin, &start_laid),
        (center_origin, &center_laid),
        (end_origin, &end_laid),
    ] {
        for laid in group {
            draw_module(
                graphics,
                laid.module,
                origin + laid.x,
                bar_height,
                font_size,
                accent_color,
            );
        }
    }
}

fn draw_module(
    graphics: &mut GraphicsEngine,
    module: &ResolvedModule,
    x: f32,
    bar_height: f32,
    font_size: f32,
    accent_color: Color,
) {
    match &module.value {
        ModuleValue::Workspaces(workspaces) => {
            let dot = workspace_dot_size(font_size);
            let gap = workspace_gap(font_size);
            let dot_y = (bar_height - dot) / 2.0;
            let mut dot_x = x;
            for ws in workspaces {
                let dot_color = if ws.is_focused {
                    accent_color
                } else if ws.is_active {
                    workspace_active_color()
                } else {
                    workspace_idle_color()
                };
                graphics.draw_rect(dot_x, dot_y, dot, dot, dot_color);
                dot_x += dot + gap;
            }
        }
        ModuleValue::Title(title) => {
            if let Some(title) = title {
                // `y = 0.0`, `line_height = bar_height`: draw_text now
                // computes the real vertically-centered baseline itself
                // (see its doc comment) instead of this call site guessing
                // an offset.
                graphics.draw_text(x, 0.0, title, font_size, bar_height, text_color());
            }
        }
        ModuleValue::SystemInfo {
            time_str,
            cpu_pct,
            mem_pct,
            battery_pct,
            ..
        } => {
            let text = format_system_info(time_str, *cpu_pct, *mem_pct, *battery_pct);
            graphics.draw_text(x, 0.0, &text, font_size, bar_height, text_color());
        }
        ModuleValue::Plugin { .. } => {
            // No live data yet; nothing to draw.
        }
    }
}

// Every dimension below is derived from `font_size` (`config.theme.font.size`,
// threaded in from `DesktopShell`) rather than being an independent
// hardcoded pixel value. There's no dedicated spacing config yet (the `shape`
// axis: spaced-pills vs. sharp-corners vs. fluid-morph will each want their own
// proportions), so these ratios are a reasonable placeholder until then.
// Promoting them to real config fields later is a matter of replacing these
// functions' bodies, not restructuring callers.
fn module_padding(font_size: f32) -> f32 {
    font_size * 0.9
}
fn module_gap(font_size: f32) -> f32 {
    font_size * 1.2
}
fn workspace_dot_size(font_size: f32) -> f32 {
    font_size * 0.75
}
fn workspace_gap(font_size: f32) -> f32 {
    font_size * 0.45
}

// Same story: no `foreground-color`/palette config exists yet, so these are
// sensible neutral defaults rather than derived from anything. `accent_color`
// (the one color that *is* real) is threaded through as a parameter instead of
// living here.
fn text_color() -> Color {
    Color::from_rgb8(235, 235, 240)
}
fn workspace_active_color() -> Color {
    Color::from_rgb8(191, 191, 199)
}
fn workspace_idle_color() -> Color {
    Color::from_rgb8(102, 102, 112)
}
