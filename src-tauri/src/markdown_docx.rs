//! Markdown (summaries, meeting notes, text pasted from an AI chat) to a Word document with real
//! formatting: headings as Word headings, lists with Word numbering, bold/italic/code, quotes and
//! tables as tables. Raw HTML in the text is written as plain text.
use docx_rs::{
    AbstractNumbering, BreakType, Docx, IndentLevel, Level, LevelJc, LevelOverride, LevelText, LineSpacing, NumberFormat,
    Numbering, NumberingId, Paragraph, Run, RunFonts, Shading, ShdType, SpecialIndentType, Start, Style, StyleType, Table,
    TableCell, TableCellMargins, TableRow, WidthType,
};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

const BULLETS: usize = 1;
const NUMBERS: usize = 2;

#[derive(Default, Clone, Copy)]
struct Marks {
    bold: bool,
    italic: bool,
    code: bool,
    strike: bool,
}

struct Builder {
    docx: Docx,
    runs: Vec<Run>,
    marks: Marks,
    heading: Option<usize>,
    /// Open lists: Word numbering id per nesting level.
    lists: Vec<usize>,
    next_num: usize,
    quote: usize,
    table: Option<Vec<(bool, Vec<Vec<Run>>)>>,
    head_row: bool,
}

fn styles(docx: Docx) -> Docx {
    let heading = |n: usize, size: usize| {
        Style::new(format!("Heading{n}"), StyleType::Paragraph)
            .name(format!("heading {n}"))
            .based_on("Normal")
            .size(size)
            .bold()
            .line_spacing(LineSpacing::new().before(if n == 1 { 120 } else { 240 }).after(80))
    };
    let level = |abstract_id: usize, i: usize| {
        let (format, text) = if abstract_id == BULLETS {
            ("bullet", ["•", "◦", "▪"][i % 3].to_string())
        } else {
            ("decimal", format!("%{}.", i + 1))
        };
        Level::new(i, Start::new(1), NumberFormat::new(format), LevelText::new(text), LevelJc::new("left"))
            .indent(Some(360 * (i as i32 + 1)), Some(SpecialIndentType::Hanging(360)), None, None)
    };
    let numbering = |id: usize| (0..6).fold(AbstractNumbering::new(id), |a, i| a.add_level(level(id, i)));
    // Calibri 11 pt with a little air after each paragraph, like a fresh Word document.
    docx.default_fonts(RunFonts::new().ascii("Calibri").hi_ansi("Calibri").cs("Calibri").east_asia("Calibri"))
        .default_size(22)
        .default_line_spacing(LineSpacing::new().after(120).line(264))
        .add_style(heading(1, 32))
        .add_style(heading(2, 28))
        .add_style(heading(3, 24))
        .add_abstract_numbering(numbering(BULLETS))
        .add_abstract_numbering(numbering(NUMBERS))
        .add_numbering(Numbering::new(BULLETS, BULLETS))
}

impl Builder {
    fn run(&self, text: &str) -> Run {
        let mut run = Run::new().add_text(text);
        if self.marks.bold || self.head_row && self.table.is_some() { run = run.bold(); }
        if self.marks.italic || self.quote > 0 { run = run.italic(); }
        if self.marks.strike { run = run.strike(); }
        if self.marks.code { run = run.fonts(RunFonts::new().ascii("Consolas").hi_ansi("Consolas")); }
        run
    }

    fn text(&mut self, text: &str) {
        let run = self.run(text);
        self.runs.push(run);
    }

    /// End the paragraph being built (list item, heading, quote or plain).
    fn paragraph(&mut self) {
        if self.runs.is_empty() || self.table.is_some() {
            return;
        }
        let mut p = std::mem::take(&mut self.runs).into_iter().fold(Paragraph::new(), Paragraph::add_run);
        if let Some(n) = self.heading {
            p = p.style(&format!("Heading{n}"));
        } else if let Some(&num) = self.lists.last() {
            p = p
                .numbering(NumberingId::new(num), IndentLevel::new(self.lists.len() - 1))
                .line_spacing(LineSpacing::new().after(40));
        } else if self.quote > 0 {
            p = p.indent(Some(567 * self.quote as i32), None, None, None);
        }
        self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                self.paragraph();
                self.heading = Some(match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    _ => 3,
                });
            }
            Event::End(TagEnd::Heading(_)) => {
                self.paragraph();
                self.heading = None;
            }
            Event::Start(Tag::Paragraph) | Event::Start(Tag::Item) => self.paragraph(),
            Event::End(TagEnd::Paragraph) | Event::End(TagEnd::Item) => self.paragraph(),
            Event::Start(Tag::List(first)) => {
                self.paragraph();
                let id = match first {
                    None => BULLETS,
                    Some(start) => {
                        // Each numbered list restarts at its own first number.
                        self.next_num += 1;
                        let id = self.next_num;
                        self.docx = std::mem::take(&mut self.docx).add_numbering(
                            Numbering::new(id, NUMBERS).add_override(LevelOverride::new(0).start(start as usize)),
                        );
                        id
                    }
                };
                self.lists.push(id);
            }
            Event::End(TagEnd::List(_)) => {
                self.paragraph();
                self.lists.pop();
            }
            Event::Start(Tag::BlockQuote(_)) => {
                self.paragraph();
                self.quote += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                self.paragraph();
                self.quote -= 1;
            }
            Event::Start(Tag::CodeBlock(_)) => {
                self.paragraph();
                self.marks.code = true;
            }
            Event::End(TagEnd::CodeBlock) => {
                self.paragraph();
                self.marks.code = false;
            }
            Event::Start(Tag::Strong) => self.marks.bold = true,
            Event::End(TagEnd::Strong) => self.marks.bold = false,
            Event::Start(Tag::Emphasis) => self.marks.italic = true,
            Event::End(TagEnd::Emphasis) => self.marks.italic = false,
            Event::Start(Tag::Strikethrough) => self.marks.strike = true,
            Event::End(TagEnd::Strikethrough) => self.marks.strike = false,
            Event::Start(Tag::Table(_)) => {
                self.paragraph();
                self.table = Some(Vec::new());
            }
            Event::Start(Tag::TableHead) => {
                self.head_row = true;
                self.table.get_or_insert_with(Vec::new).push((true, Vec::new()));
            }
            Event::End(TagEnd::TableHead) => self.head_row = false,
            Event::Start(Tag::TableRow) => self.table.get_or_insert_with(Vec::new).push((false, Vec::new())),
            Event::Start(Tag::TableCell) => {
                self.runs.clear();
            }
            Event::End(TagEnd::TableCell) => {
                let runs = std::mem::take(&mut self.runs);
                if let Some(row) = self.table.as_mut().and_then(|t| t.last_mut()) {
                    row.1.push(runs);
                }
            }
            Event::End(TagEnd::Table) => {
                let rows = self.table.take().unwrap_or_default();
                let width = rows.iter().map(|r| r.1.len()).max().unwrap_or(0);
                if width > 0 {
                    let tight = || LineSpacing::new().before(0).after(0);
                    let rows: Vec<TableRow> = rows
                        .into_iter()
                        .map(|(head, cells)| {
                            let mut cells: Vec<TableCell> = cells
                                .into_iter()
                                .map(|runs| {
                                    let cell = TableCell::new().add_paragraph(
                                        runs.into_iter().fold(Paragraph::new().line_spacing(tight()), Paragraph::add_run),
                                    );
                                    if head { cell.shading(Shading::new().shd_type(ShdType::Clear).fill("F1F1EE")) } else { cell }
                                })
                                .collect();
                            cells.resize_with(width, || TableCell::new().add_paragraph(Paragraph::new()));
                            TableRow::new(cells)
                        })
                        .collect();
                    let grid = vec![9000 / width; width];
                    let margins = TableCellMargins::new()
                        .margin_top(60, WidthType::Dxa)
                        .margin_bottom(60, WidthType::Dxa)
                        .margin_left(100, WidthType::Dxa)
                        .margin_right(100, WidthType::Dxa);
                    let table = Table::new(rows).set_grid(grid).width(5000, WidthType::Pct).margins(margins);
                    self.docx = std::mem::take(&mut self.docx).add_table(table).add_paragraph(Paragraph::new());
                }
            }
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                if self.marks.code && self.table.is_none() {
                    // Code blocks: one paragraph per line.
                    let mut lines = text.split('\n').peekable();
                    while let Some(line) = lines.next() {
                        self.text(line);
                        if lines.peek().is_some() {
                            self.paragraph();
                        }
                    }
                } else {
                    self.text(&text);
                }
            }
            Event::Code(text) => {
                let saved = self.marks;
                self.marks.code = true;
                self.text(&text);
                self.marks = saved;
            }
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.runs.push(Run::new().add_break(BreakType::TextWrapping)),
            Event::Rule => {
                self.paragraph();
                self.docx = std::mem::take(&mut self.docx).add_paragraph(Paragraph::new());
            }
            Event::TaskListMarker(done) => self.text(if done { "☑ " } else { "☐ " }),
            _ => {}
        }
    }
}

/// Build a Word document from markdown.
pub fn build(markdown: &str) -> Docx {
    let mut builder = Builder {
        docx: styles(Docx::new()),
        runs: Vec::new(),
        marks: Marks::default(),
        heading: None,
        lists: Vec::new(),
        next_num: NUMBERS,
        quote: 0,
        table: None,
        head_row: false,
    };
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for event in Parser::new_ext(markdown, options) {
        builder.event(event);
    }
    builder.paragraph();
    builder.docx
}

#[cfg(test)]
mod tests {
    use super::*;
    fn xml(markdown: &str) -> String {
        let mut out = std::io::Cursor::new(Vec::new());
        build(markdown).build().pack(&mut out).unwrap();
        let doc = docx_rs::read_docx(out.get_ref()).unwrap();
        serde_json::to_string(&doc.document).unwrap()
    }
    #[test]
    fn headings_lists_emphasis_and_tables_become_word_formatting() {
        let x = xml("# Möte\n\n## Beslut\n\n- **Budget** klar\n- Schema *senare*\n\n1. Första\n2. Andra\n\n| Vem | Vad |\n|---|---|\n| Anna | Protokoll |\n| Karim | Lokal |\n");
        assert!(x.contains("Heading1") && x.contains("Heading2"), "headings use Word heading styles");
        assert!(x.contains("\"numberingProperty\""), "list items use Word numbering");
        assert!(x.contains("\"type\":\"table\""), "a real table");
        for word in ["Möte", "Budget", "Protokoll", "Karim"] {
            assert!(x.contains(word), "{word} kept");
        }
        assert!(!x.contains("| Vem"), "no markdown table syntax in the document");
        assert!(!x.contains("**"), "no markdown emphasis marks");
    }
    /// Writes a sample for looking at in Word: AVSKRIFT_MARKDOWN_SAMPLE=<path.docx>. Opt-in.
    #[test]
    #[ignore]
    fn write_sample() {
        let path = std::env::var("AVSKRIFT_MARKDOWN_SAMPLE").unwrap();
        let md = "# Veckomöte 3 oktober

Sammanfattning gjord med **extern AI** och *granskad*.

## Beslut

1. Budgeten för läsfrämjande insatser ligger kvar.
2. Uppföljning på nästa möte.
   - Anna tar fram underlag
   - Karim bokar lokal

## Åtgärder

| Vem | Vad | När |
|---|---|---|
| Anna | Underlag för läsfrämjande | 15 okt |
| Karim | Boka lokal | 10 okt |

> Citat från mötet: \"Vi börjar i årskurs fyra.\"

Kod: `ggml-model-q5_0.bin`
";
        crate::docio::save_built(std::path::Path::new(&path), build(md)).unwrap();
    }
    #[test]
    fn raw_html_is_text_and_plain_lines_survive() {
        let x = xml("Hej <script>alert(1)</script>\n\nRad två");
        assert!(x.contains("Rad två"));
        assert!(x.contains("script"), "html is kept as literal text, never interpreted");
    }
}
