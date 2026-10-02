use crate::world::MemoryWorld;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use typst::diag::{SourceDiagnostic, Warned};

/// Compiler errors with the byte range they refer to in the main source, if known.
pub type Diagnostics = Vec<(SourceDiagnostic, Option<std::ops::Range<usize>>)>;
use typst::layout::{Frame, FrameItem};
use typst::utils::Scalar;
use typst_html::{HtmlDocument, HtmlOptions};
use typst_layout::PagedDocument;
use typst_pdf::{pdf, PdfOptions};
use typst_render::{render, RenderOptions};
use typst_svg::SvgOptions;

#[derive(Serialize, Clone)]
pub struct DocumentStats {
    pub pages: usize,
    pub words: usize,
    pub characters: usize,
    pub characters_excluding_spaces: usize,
}

fn extract_stats(doc: &PagedDocument) -> DocumentStats {
    let mut text = String::new();
    let pages = doc.pages().len();
    for page in doc.pages() {
        extract_frame_text(&page.frame, &mut text);
    }

    let words = text.split_whitespace().count();
    let characters = text.chars().count();
    let characters_excluding_spaces = text.chars().filter(|c| !c.is_whitespace()).count();

    DocumentStats {
        pages,
        words,
        characters,
        characters_excluding_spaces,
    }
}

fn extract_frame_text(frame: &Frame, text: &mut String) {
    for (_, item) in frame.items() {
        match item {
            FrameItem::Text(text_item) => {
                text.push_str(&text_item.text);
                text.push(' ');
            }
            FrameItem::Group(group) => {
                extract_frame_text(&group.frame, text);
            }
            _ => {}
        }
    }
}

/// One page of the live preview. `svg` is absent when the client already has
/// a page with this hash.
#[derive(Serialize)]
pub struct PreviewPage {
    pub hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub svg: Option<String>,
}

pub struct ProjectInput {
    pub entrypoint: String,
    pub files: HashMap<String, Vec<u8>>,
    pub packages: HashMap<String, HashMap<String, Vec<u8>>>,
}

impl ProjectInput {
    pub fn single(text: String, files: HashMap<String, Vec<u8>>) -> Self {
        let mut project_files = files;
        project_files.insert("main.typ".to_string(), text.into_bytes());
        Self {
            entrypoint: "main.typ".to_string(),
            files: project_files,
            packages: HashMap::new(),
        }
    }

    fn into_world(self, enable_html: bool) -> MemoryWorld {
        MemoryWorld::new_project(self.entrypoint, self.files, self.packages, enable_html)
    }
}

pub struct TypstCompiler;

impl TypstCompiler {
    pub fn new() -> Self {
        Self
    }

    pub fn compile_svg(
        &self,
        input: ProjectInput,
    ) -> Result<
        (Vec<String>, String, DocumentStats),
        Diagnostics,
    > {
        let world = input.into_world(false);
        match typst::compile::<PagedDocument>(&world) {
            Warned {
                output: Ok(doc),
                warnings: _,
            } => {
                let stats = extract_stats(&doc);
                let options = SvgOptions::default();
                let svgs: Vec<String> = doc
                    .pages()
                    .iter()
                    .map(|page| typst_svg::svg(page, &options))
                    .collect();
                let thumbnail = svgs.first().cloned().unwrap_or_default();
                Ok((svgs, thumbnail, stats))
            }
            Warned {
                output: Err(errors),
                warnings: _,
            } => {
                use typst::WorldExt;
                let diag = errors
                    .into_iter()
                    .map(|d| {
                        let range = world.range(d.span);
                        (d, range)
                    })
                    .collect();
                Err(diag)
            }
        }
    }

    /// Compiles for the live preview. Every page is identified by a hash of its
    /// layout; pages whose hash the client already has (`known`) are not
    /// rendered again, since rendering and shipping SVG is the expensive part.
    pub fn compile_preview(
        &self,
        input: ProjectInput,
        known: &HashSet<String>,
    ) -> Result<(Vec<PreviewPage>, DocumentStats), Diagnostics> {
        let world = input.into_world(false);
        match typst::compile::<PagedDocument>(&world) {
            Warned { output: Ok(doc), warnings: _ } => {
                let stats = extract_stats(&doc);
                let options = SvgOptions::default();
                let pages = doc
                    .pages()
                    .iter()
                    .map(|page| {
                        let hash = format!("{:032x}", typst::utils::hash128(page));
                        let svg = (!known.contains(&hash)).then(|| typst_svg::svg(page, &options));
                        PreviewPage { hash, svg }
                    })
                    .collect();
                Ok((pages, stats))
            }
            Warned { output: Err(errors), warnings: _ } => {
                use typst::WorldExt;
                Err(errors
                    .into_iter()
                    .map(|d| {
                        let range = world.range(d.span);
                        (d, range)
                    })
                    .collect())
            }
        }
    }

    pub fn export_pdf(
        &self,
        input: ProjectInput,
    ) -> Result<Vec<u8>, Diagnostics> {
        let world = input.into_world(false);
        match typst::compile::<PagedDocument>(&world) {
            Warned {
                output: Ok(doc),
                warnings: _,
            } => {
                let opts = PdfOptions::default();
                match pdf(&doc, &opts) {
                    Ok(bytes) => Ok(bytes),
                    Err(_) => Err(vec![]),
                }
            }
            Warned {
                output: Err(errors),
                warnings: _,
            } => {
                use typst::WorldExt;
                Err(errors
                    .into_iter()
                    .map(|d| {
                        let range = world.range(d.span);
                        (d, range)
                    })
                    .collect())
            }
        }
    }

    pub fn export_png(
        &self,
        input: ProjectInput,
    ) -> Result<Vec<u8>, Diagnostics> {
        let world = input.into_world(false);
        match typst::compile::<PagedDocument>(&world) {
            Warned {
                output: Ok(doc),
                warnings: _,
            } => {
                if let Some(page) = doc.pages().first() {
                    let options = RenderOptions {
                        pixel_per_pt: Scalar::new(2.0),
                        ..RenderOptions::default()
                    };
                    let pixmap = render(page, &options);
                    if let Ok(encoded) = pixmap.encode_png() {
                        return Ok(encoded);
                    }
                }
                Ok(vec![])
            }
            Warned {
                output: Err(errors),
                warnings: _,
            } => {
                use typst::WorldExt;
                Err(errors
                    .into_iter()
                    .map(|d| {
                        let range = world.range(d.span);
                        (d, range)
                    })
                    .collect())
            }
        }
    }

    pub fn export_html(
        &self,
        input: ProjectInput,
    ) -> Result<String, Diagnostics> {
        let world = input.into_world(true);
        let document = match typst::compile::<HtmlDocument>(&world) {
            Warned {
                output: Ok(document),
                warnings: _,
            } => document,
            Warned {
                output: Err(errors),
                warnings: _,
            } => {
                use typst::WorldExt;
                return Err(errors
                    .into_iter()
                    .map(|d| {
                        let range = world.range(d.span);
                        (d, range)
                    })
                    .collect());
            }
        };

        match typst_html::html(&document, &HtmlOptions::default()) {
            Ok(html) => Ok(html),
            Err(errors) => {
                use typst::WorldExt;
                Err(errors
                    .into_iter()
                    .map(|d| {
                        let range = world.range(d.span);
                        (d, range)
                    })
                    .collect())
            }
        }
    }
}
