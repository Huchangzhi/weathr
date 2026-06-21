use clap::Parser;
use clap::builder::{PossibleValue, PossibleValuesParser};
use clap_complete::Shell;

use crate::weather::WeatherCondition;

const LONG_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    "\n\n天气数据由 Open-Meteo.com (https://open-meteo.com/) 提供\n",
    "数据许可: CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/)\n\n",
    "地理编码由 Nominatim/OpenStreetMap (https://nominatim.openstreetmap.org/) 提供\n",
    "数据 \u{00a9} OpenStreetMap 贡献者, ODbL (https://www.openstreetmap.org/copyright)"
);

const ABOUT: &str = concat!(
    "基于终端的 ASCII 天气动画应用\n\n",
    "天气数据由 Open-Meteo.com (https://open-meteo.com/) 提供\n",
    "数据许可: CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/)\n\n",
    "地理编码由 Nominatim/OpenStreetMap (https://nominatim.openstreetmap.org/) 提供\n",
    "数据 \u{00a9} OpenStreetMap 贡献者, ODbL (https://www.openstreetmap.org/copyright)"
);

fn simulate_parser() -> PossibleValuesParser {
    PossibleValuesParser::new(
        WeatherCondition::ALL
            .iter()
            .map(|c| PossibleValue::new(c.as_str()).help(c.description())),
    )
}

#[derive(Parser)]
#[command(version, long_version = LONG_VERSION, about = ABOUT, long_about = None)]
pub struct Cli {
    #[arg(
        short,
        long,
        value_name = "CONDITION",
        value_parser = simulate_parser(),
        help = "模拟天气状况 (clear, rain, drizzle, snow 等)"
    )]
    pub simulate: Option<String>,

    #[arg(
        short,
        long,
        help = "模拟夜间 (用于测试月亮、星星、萤火虫)"
    )]
    pub night: bool,

    #[arg(short, long, help = "启用秋季落叶动画")]
    pub leaves: bool,

    #[arg(long, help = "通过 IP 自动检测位置 (使用 ipinfo.io)")]
    pub auto_location: bool,

    #[arg(long, help = "在界面中隐藏位置坐标")]
    pub hide_location: bool,

    #[arg(long, help = "隐藏 HUD (状态栏)")]
    pub hide_hud: bool,

    #[arg(
        long,
        conflicts_with = "metric",
        help = "使用英制单位 (°F, mph, inch)"
    )]
    pub imperial: bool,

    #[arg(
        long,
        conflicts_with = "imperial",
        help = "使用公制单位 (°C, km/h, mm)"
    )]
    pub metric: bool,

    #[arg(long, help = "静默运行 (抑制非错误输出)")]
    pub silent: bool,

    #[arg(long, value_name = "SHELL", value_enum)]
    pub completions: Option<Shell>,
}

pub fn extract_simulate_missing_value(err: clap::Error) -> clap::Error {
    let msg = err.to_string();
    if msg.contains("--simulate") && msg.contains("value is required") {
        err
    } else {
        err.exit()
    }
}

pub fn print_simulate_help() {
    let mut current_group = "";

    eprintln!("可用的天气状况:");
    for condition in WeatherCondition::ALL {
        let group = condition.group();
        if group != current_group {
            eprintln!();
            eprintln!("  {}:", group);
            current_group = group;
        }
        eprintln!(
            "    {:<18} - {}",
            condition.as_str(),
            condition.description()
        );
    }

    eprintln!();
    eprintln!("示例:");
    eprintln!("  weathr --simulate rain");
    eprintln!("  weathr --simulate snow --night");
    eprintln!("  weathr -s thunderstorm -n");
}
