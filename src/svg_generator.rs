use anyhow::Result;
use std::{fmt::Write, fs, path::Path};

use crate::stats::{LanguageInfo, Stats};

const MAX_LANGUAGES: usize = 12;

pub struct SvgGenerator;

impl SvgGenerator {
    pub fn generate_overview(stats: &Stats) -> Result<()> {
        // Read template
        let template = fs::read_to_string("templates/overview.svg")?;

        // Replace placeholders
        let output = template
            .replace("{{ name }}", &stats.name)
            .replace("{{ stars }}", &format_number(stats.total_stars))
            .replace("{{ forks }}", &format_number(stats.total_forks))
            .replace(
                "{{ contributions }}",
                &format_number(stats.total_contributions),
            )
            .replace(
                "{{ lines_changed }}",
                &format_number(stats.lines_added + stats.lines_deleted),
            )
            .replace("{{ views }}", &format_number(stats.total_views))
            .replace("{{ repos }}", &format_number(stats.total_repos as u64));

        // Create output directory if it doesn't exist
        if !Path::new("generated").exists() {
            fs::create_dir("generated")?;
        }

        // Write output
        fs::write("generated/overview.svg", output)?;
        Ok(())
    }

    pub fn generate_languages(stats: &Stats) -> Result<()> {
        // Read template
        let template = fs::read_to_string("templates/languages.svg")?;

        // Sort languages by size
        let mut languages: Vec<(&String, &LanguageInfo)> = stats.languages.iter().collect();
        languages.sort_by(|a, b| b.1.size.cmp(&a.1.size));

        // Generate progress bar and language list
        let mut progress = String::new();
        let mut lang_list = String::new();
        let delay_between = 150;

        // Calculate how many languages fit
        // foreignObject height: 176px
        // Header (h2): ~36px (16px font + 24px line-height + margin)
        // Progress bar: ~22px (8px height + 1em margin)
        // Available for languages: ~118px
        // Each row: 21px (line-height)
        // Maximum rows: 5 (118px / 21px = 5.6)
        // With wrapping, we need to limit total to avoid overflow

        for (i, (name, info)) in languages.iter().take(MAX_LANGUAGES).enumerate() {
            let color = info.color.as_deref().unwrap_or("#000000");

            write!(
                progress,
                r#"<span style="background-color: {};width: {:.3}%;" class="progress-item"></span>"#,
                color, info.percentage
            )?;

            write!(
                lang_list,
                r#"
<li style="animation-delay: {}ms;">
<svg xmlns="http://www.w3.org/2000/svg" class="octicon" style="fill:{};"
viewBox="0 0 16 16" version="1.1" width="16" height="16"><path
fill-rule="evenodd" d="M8 4a4 4 0 100 8 4 4 0 000-8z"></path></svg>
<span class="lang">{}</span>
<span class="percent">{:.2}%</span>
</li>
"#,
                i * delay_between,
                color,
                name,
                info.percentage
            )?;
        }

        // Replace placeholders
        let output = template
            .replace("{{ progress }}", &progress)
            .replace("{{ lang_list }}", &lang_list);

        // Write output
        fs::write("generated/languages.svg", output)?;
        Ok(())
    }
}

fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();

    for (count, ch) in s.chars().rev().enumerate() {
        if count > 0 && count % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }

    result.chars().rev().collect()
}
