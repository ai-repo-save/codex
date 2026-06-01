//! Skill-load history cells.

use super::*;

#[derive(Debug)]
pub(crate) struct SkillLoadCell {
    name: String,
    path: Option<AbsolutePathBuf>,
    status: codex_app_server_protocol::SkillLoadStatus,
    error: Option<String>,
}

impl HistoryCell for SkillLoadCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let failed = matches!(
            self.status,
            codex_app_server_protocol::SkillLoadStatus::Failed
        );
        let bullet = if failed {
            "•".red().bold()
        } else {
            "•".green().bold()
        };
        let header = if failed {
            format!("Failed to read skill {}", self.name)
        } else {
            format!("Read skill {}", self.name)
        };
        lines.push(Line::from(vec![bullet, " ".into(), header.bold()]));

        let detail_wrap_width = (width as usize).saturating_sub(4).max(1);
        let mut detail_lines = Vec::new();
        if let Some(path) = &self.path {
            let line = Line::from(path.display().to_string().dim());
            let wrapped = adaptive_wrap_line(
                &line,
                RtOptions::new(detail_wrap_width)
                    .initial_indent("".into())
                    .subsequent_indent("    ".into()),
            );
            detail_lines.extend(wrapped.iter().map(line_to_static));
        }
        if let Some(error) = &self.error {
            let line = Line::from(error.clone().dim());
            let wrapped = adaptive_wrap_line(
                &line,
                RtOptions::new(detail_wrap_width)
                    .initial_indent("".into())
                    .subsequent_indent("    ".into()),
            );
            detail_lines.extend(wrapped.iter().map(line_to_static));
        }
        lines.extend(prefix_lines(detail_lines, "  └ ".dim(), "    ".into()));
        lines
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        if matches!(
            self.status,
            codex_app_server_protocol::SkillLoadStatus::Failed
        ) {
            lines.push(Line::from(format!("Failed to read skill {}", self.name)));
        } else {
            lines.push(Line::from(format!("Read skill {}", self.name)));
        }
        if let Some(path) = &self.path {
            lines.push(Line::from(path.display().to_string()));
        }
        if let Some(error) = &self.error {
            lines.push(Line::from(error.clone()));
        }
        lines
    }
}

pub(crate) fn new_skill_load(
    name: String,
    path: Option<AbsolutePathBuf>,
    status: codex_app_server_protocol::SkillLoadStatus,
    error: Option<String>,
) -> SkillLoadCell {
    SkillLoadCell {
        name,
        path,
        status,
        error,
    }
}
