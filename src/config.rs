use std::path::PathBuf;

use facet::Facet;
use facet_default as default;

#[derive(Facet, Debug, Clone)]
#[facet(derive(Default))]
pub struct Config {
    pub theme: ThemeConfig,
    pub bar: BarConfig,
    pub launcher: LauncherConfig,
    pub notifications: NotificationsConfig,
    pub lock: LockConfig,
    pub plugins: Vec<PluginConfig>,
}

#[derive(Facet, Debug, Clone)]
#[facet(derive(Default))]
pub struct ThemeConfig {
    pub font: FontConfig,
    #[facet(rename = "background-blur")]
    pub background_blur: BackgroundBlurConfig,
    #[facet(rename = "accent-color", default = "#ff007f")]
    pub accent_color: String,
    pub material: MaterialConfig,
    pub shape: ShapeConfig,
    pub colorway: ColorwayConfig,
    #[facet(rename = "reduced-motion", default = false)]
    pub reduced_motion: bool,
    #[facet(rename = "reduced-transparency", default = false)]
    pub reduced_transparency: bool,
}

#[repr(u8)]
#[derive(Facet, Debug, Clone, PartialEq, Eq)]
#[facet(derive(Default), rename_all = "kebab-case")]
pub enum MaterialConfig {
    #[facet(default::variant)]
    Flat,
    FrostedGlass,
    LiquidGlass,
}

#[repr(u8)]
#[derive(Facet, Debug, Clone, PartialEq, Eq)]
#[facet(derive(Default), rename_all = "kebab-case")]
pub enum ShapeConfig {
    #[facet(default::variant)]
    RoundedRect,
    SpacedPills,
    SharpCorners,
    FluidMorph,
}

#[repr(u8)]
#[derive(Facet, Debug, Clone, PartialEq, Eq)]
#[facet(derive(Default), rename_all = "kebab-case")]
pub enum ColorwayConfig {
    #[facet(default::variant)]
    Accent,
    HoloWhite,
    Obsidian,
}

#[derive(Facet, Debug, Clone)]
#[facet(derive(Default))]
pub struct FontConfig {
    #[facet(default = "Sans")]
    pub name: String,
    #[facet(default = 11.0_f32)]
    pub size: f32,
}

#[derive(Facet, Debug, Clone)]
#[facet(derive(Default))]
pub struct BackgroundBlurConfig {
    #[facet(default = 10.0_f32)]
    pub radius: f32,
    #[facet(default = 0.5_f32)]
    pub intensity: f32,
}

#[derive(Facet, Debug, Clone)]
#[facet(derive(Default))]
pub struct BarConfig {
    pub position: BarConfigPosition,
    #[facet(default = 32_u32)]
    pub height: u32,
    pub modules: Vec<BarModuleConfig>,
}

#[repr(u8)]
#[derive(Facet, Debug, Clone, PartialEq, Eq)]
#[facet(derive(Default), rename_all = "kebab-case")]
pub enum BarConfigPosition {
    #[facet(default::variant)]
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Facet, Debug, Clone)]
#[facet(derive(Default))]
pub struct BarModuleConfig {
    #[facet(rename = "type")]
    pub module_type: BarModuleConfigType,
    pub id: Option<String>,
    pub align: BarModuleConfigAlign,
}

#[repr(u8)]
#[derive(Facet, Debug, Clone, PartialEq, Eq)]
#[facet(derive(Default), rename_all = "kebab-case")]
pub enum BarModuleConfigType {
    #[facet(default::variant)]
    Workspaces,
    Title,
    SystemInfo,
    Plugin,
}

#[repr(u8)]
#[derive(Facet, Debug, Clone, PartialEq, Eq)]
#[facet(derive(Default), rename_all = "kebab-case")]
pub enum BarModuleConfigAlign {
    #[facet(default::variant)]
    Start,
    Center,
    End,
}

#[derive(Facet, Debug, Clone)]
#[facet(derive(Default))]
pub struct LauncherConfig {
    #[facet(default = "Super+D")]
    pub hotkey: String,
    #[facet(rename = "show-icons", default = false)]
    pub show_icons: bool,
    #[facet(rename = "fuzzy-match")]
    pub fuzzy_match: LauncherConfigFuzzyMatch,
}

#[repr(u8)]
#[derive(Facet, Debug, Clone, PartialEq, Eq)]
#[facet(derive(Default), rename_all = "kebab-case")]
pub enum LauncherConfigFuzzyMatch {
    #[facet(default::variant)]
    SmartCase,
    CaseSensitive,
    CaseInsensitive,
}

#[derive(Facet, Debug, Clone)]
#[facet(derive(Default))]
pub struct NotificationsConfig {
    #[facet(default = 5000_u32)]
    pub timeout: u32,
    #[facet(rename = "max-visible", default = 5_u32)]
    pub max_visible: u32,
    pub anchor: NotificationsConfigAnchor,
    #[facet(rename = "on-click")]
    pub on_click: NotificationsConfigOnClick,
}

#[repr(u8)]
#[derive(Facet, Debug, Clone, PartialEq, Eq)]
#[facet(derive(Default), rename_all = "kebab-case")]
pub enum NotificationsConfigAnchor {
    #[facet(default::variant)]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[repr(u8)]
#[derive(Facet, Debug, Clone, PartialEq, Eq)]
#[facet(derive(Default), rename_all = "kebab-case")]
pub enum NotificationsConfigOnClick {
    #[facet(default::variant)]
    Dismiss,
    Open,
}

#[derive(Facet, Debug, Clone)]
#[facet(derive(Default))]
pub struct LockConfig {
    #[facet(rename = "blur-background", default = true)]
    pub blur_background: bool,
    #[facet(rename = "input-echo", default = false)]
    pub input_echo: bool,
    #[facet(rename = "on-suspend")]
    pub on_suspend: LockConfigOnSuspend,
}

#[repr(u8)]
#[derive(Facet, Debug, Clone, PartialEq, Eq)]
#[facet(derive(Default), rename_all = "kebab-case")]
pub enum LockConfigOnSuspend {
    #[facet(default::variant)]
    Lock,
    Logout,
    Suspend,
    Hibernate,
}

#[derive(Facet, Debug, Clone)]
#[facet(derive(Default))]
pub struct PluginConfig {
    pub id: String,
    pub path: PathBuf,
    #[facet(default = 60_u32)]
    pub interval: u32,
    #[facet(rename = "allow-network")]
    pub allow_network: Vec<String>,
}

pub fn parse_config(content: &str) -> Result<Config, String> {
    facet_styx::from_str(content).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_styx() {
        let content = r#"
            theme {
                font {
                    name Inter
                    size 11.0
                }
                background-blur {
                    radius 15.0
                    intensity 0.8
                }
                accent-color #ff007f
                material @frosted-glass
                shape @spaced-pills
                colorway @holo-white
                reduced-motion false
                reduced-transparency false
            }
            bar {
                position @top
                height 32
                modules (
                    { type @workspaces, align @start }
                    { type @title, align @center }
                    { type @system-info, align @end }
                    { type @plugin, id weather-widget, align @end }
                )
            }
            launcher {
                hotkey Super+D
                show-icons true
                fuzzy-match @smart-case
            }
            notifications {
                timeout 5000
                max-visible 5
                anchor @top-right
                on-click @dismiss
            }
            lock {
                blur-background true
                input-echo true
                on-suspend lock
            }
            plugins (
                {
                    id weather-widget
                    path ~/.config/nacre/plugins/weather.wasm
                    interval 1800
                    allow-network ( api.open-meteo.com )
                }
            )
        "#;

        let config = parse_config(content).expect("Failed to parse config");
        assert_eq!(config.theme.font.name, "Inter");
        assert_eq!(config.theme.font.size, 11.0);
        assert_eq!(config.theme.background_blur.radius, 15.0);
        assert_eq!(config.theme.background_blur.intensity, 0.8);
        assert_eq!(config.theme.accent_color, "#ff007f");
        assert_eq!(config.theme.material, MaterialConfig::FrostedGlass);
        assert_eq!(config.theme.shape, ShapeConfig::SpacedPills);
        assert_eq!(config.theme.colorway, ColorwayConfig::HoloWhite);
        assert!(!config.theme.reduced_motion);
        assert!(!config.theme.reduced_transparency);

        assert_eq!(config.bar.position, BarConfigPosition::Top);
        assert_eq!(config.bar.height, 32);
        assert_eq!(config.bar.modules.len(), 4);
        assert_eq!(
            config.bar.modules[0].module_type,
            BarModuleConfigType::Workspaces
        );
        assert_eq!(config.bar.modules[0].align, BarModuleConfigAlign::Start);
        assert_eq!(config.bar.modules[3].id.as_deref(), Some("weather-widget"));

        assert_eq!(config.launcher.hotkey, "Super+D");
        assert!(config.launcher.show_icons);
        assert_eq!(
            config.launcher.fuzzy_match,
            LauncherConfigFuzzyMatch::SmartCase
        );

        assert_eq!(config.notifications.timeout, 5000);
        assert_eq!(config.notifications.max_visible, 5);
        assert_eq!(
            config.notifications.anchor,
            NotificationsConfigAnchor::TopRight
        );

        assert!(config.lock.blur_background);
        assert!(config.lock.input_echo);
        assert_eq!(config.lock.on_suspend, LockConfigOnSuspend::Lock);

        assert_eq!(config.plugins.len(), 1);
        assert_eq!(config.plugins[0].id, "weather-widget");
        assert_eq!(config.plugins[0].interval, 1800);
        assert_eq!(config.plugins[0].allow_network[0], "api.open-meteo.com");
    }
}
