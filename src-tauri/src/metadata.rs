use std::fs::File;
use std::io::{Cursor, Read};
use std::path::{Component, Path};

use roxmltree::Document;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookFormat {
    Pdf,
    Epub,
}

impl BookFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Epub => "epub",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pdf => "PDF",
            Self::Epub => "EPUB",
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "pdf" => Some(Self::Pdf),
            "epub" => Some(Self::Epub),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Extracted {
    pub title: String,
    pub author: String,
    pub year: Option<i32>,
    pub format: BookFormat,
    pub cover_png: Vec<u8>,
}

#[derive(Debug)]
pub struct EpubDetails {
    pub title: String,
    pub author: String,
    pub year: Option<i32>,
    pub cover: Option<Vec<u8>>,
}

#[derive(Debug)]
pub enum ExtractError {
    Io(std::io::Error),
    Zip(zip::result::ZipError),
    Xml(String),
    Pdf(String),
    Image(String),
    Render(String),
    Unsupported,
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Zip(err) => write!(f, "{err}"),
            Self::Xml(err) | Self::Pdf(err) | Self::Image(err) | Self::Render(err) => {
                write!(f, "{err}")
            }
            Self::Unsupported => write!(f, "Choose a PDF or EPUB file"),
        }
    }
}

impl std::error::Error for ExtractError {}

impl From<std::io::Error> for ExtractError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<zip::result::ZipError> for ExtractError {
    fn from(value: zip::result::ZipError) -> Self {
        Self::Zip(value)
    }
}

pub fn extract(path: &Path) -> Result<Extracted, ExtractError> {
    let format = BookFormat::from_path(path).ok_or(ExtractError::Unsupported)?;
    let extracted = match format {
        BookFormat::Pdf => extract_pdf(path)?,
        BookFormat::Epub => {
            let details = inspect_epub(path)?;
            let cover_png = match details.cover.as_deref().and_then(|bytes| png_from_bytes(bytes).ok())
            {
                Some(png) => png,
                None => title_card_png(&details.title, &details.author)?,
            };
            Extracted {
                title: details.title,
                author: details.author,
                year: details.year,
                format,
                cover_png,
            }
        }
    };
    Ok(extracted)
}

pub fn png_from_bytes(bytes: &[u8]) -> Result<Vec<u8>, ExtractError> {
    let image = image::load_from_memory(bytes).map_err(|err| ExtractError::Image(err.to_string()))?;
    let image = if image.width() > 400 {
        let height = ((image.height() as f32) * 400.0 / (image.width() as f32)).round() as u32;
        image.resize(400, height.max(1), image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|err| ExtractError::Image(err.to_string()))?;
    Ok(png)
}

pub fn title_card_png(title: &str, author: &str) -> Result<Vec<u8>, ExtractError> {
    let width = 400;
    let height = 600;
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, width, height)
        .map_err(|err| ExtractError::Render(err.to_string()))?;
    let context = cairo::Context::new(&surface).map_err(|err| ExtractError::Render(err.to_string()))?;

    context.set_source_rgb(0.93, 0.91, 0.86);
    context.paint().map_err(|err| ExtractError::Render(err.to_string()))?;
    context.set_source_rgb(0.22, 0.27, 0.32);
    context.rectangle(0.0, 0.0, 16.0, height as f64);
    context.fill().map_err(|err| ExtractError::Render(err.to_string()))?;

    draw_text(&context, title, 36.0, 72.0, 340.0, 28, true);
    if !author.trim().is_empty() {
        draw_text(&context, author, 36.0, 470.0, 340.0, 18, false);
    }

    surface.flush();
    let mut png = Vec::new();
    surface
        .write_to_png(&mut png)
        .map_err(|err| ExtractError::Render(err.to_string()))?;
    Ok(png)
}

fn draw_text(context: &cairo::Context, text: &str, x: f64, y: f64, width: f64, size: i32, bold: bool) {
    let layout = pangocairo::functions::create_layout(context);
    layout.set_width((width as i32) * pango::SCALE);
    layout.set_wrap(pango::WrapMode::WordChar);
    layout.set_text(text);
    let mut font = pango::FontDescription::new();
    font.set_family("Sans");
    font.set_size(size * pango::SCALE);
    font.set_weight(if bold {
        pango::Weight::Bold
    } else {
        pango::Weight::Normal
    });
    layout.set_font_description(Some(&font));
    context.set_source_rgb(0.16, 0.17, 0.19);
    context.move_to(x, y);
    pangocairo::functions::show_layout(context, &layout);
}

fn file_uri(path: &Path) -> String {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let mut uri = String::from("file://");
    for component in absolute.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(part) => {
                uri.push('/');
                for byte in part.to_string_lossy().bytes() {
                    match byte {
                        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                            uri.push(byte as char);
                        }
                        _ => uri.push_str(&format!("%{byte:02X}")),
                    }
                }
            }
            Component::CurDir => {}
            Component::ParentDir => uri.push_str("/.."),
            Component::Prefix(_) => {}
        }
    }
    if uri == "file://" {
        uri.push('/');
    }
    uri
}

fn year_from_unix_local(timestamp: i64) -> Option<i32> {
    let seconds = timestamp as libc::time_t;
    let mut date = unsafe { std::mem::zeroed::<libc::tm>() };
    if unsafe { libc::localtime_r(&seconds, &mut date) }.is_null() {
        return None;
    }
    let year = date.tm_year + 1900;
    (1000..=9999).contains(&year).then_some(year)
}

fn extract_pdf(path: &Path) -> Result<Extracted, ExtractError> {
    let uri = file_uri(path);
    let document = poppler::Document::from_file(uri.as_str(), None)
        .map_err(|err| ExtractError::Pdf(err.to_string()))?;
    let title = clean_meta(document.title().as_deref()).unwrap_or_else(|| fallback_title(path));
    let author = clean_meta(document.author().as_deref()).unwrap_or_default();
    let year = pdf_year(&document);
    let cover_png = render_pdf_page(&document).unwrap_or_else(|_| {
        title_card_png(&title, &author).unwrap_or_else(|_| Vec::new())
    });
    Ok(Extracted {
        title,
        author,
        year,
        format: BookFormat::Pdf,
        cover_png,
    })
}

fn pdf_year(document: &poppler::Document) -> Option<i32> {
    let timestamp = document.creation_date();
    if timestamp > 0 {
        if let Some(year) = year_from_unix_local(timestamp as i64) {
            return Some(year);
        }
    }
    document.metadata().and_then(|metadata| year_in_text(&metadata))
}

fn render_pdf_page(document: &poppler::Document) -> Result<Vec<u8>, ExtractError> {
    let page = document
        .page(0)
        .ok_or_else(|| ExtractError::Pdf("This PDF has no pages".to_string()))?;
    let (page_width, page_height) = page.size();
    if page_width < 1.0 || page_height < 1.0 {
        return Err(ExtractError::Pdf("This PDF page has no size".to_string()));
    }
    let target_width = 400.0;
    let scale = target_width / page_width;
    let target_height = (page_height * scale).round().max(1.0) as i32;
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, target_width as i32, target_height)
        .map_err(|err| ExtractError::Render(err.to_string()))?;
    let context = cairo::Context::new(&surface).map_err(|err| ExtractError::Render(err.to_string()))?;
    context.set_source_rgb(1.0, 1.0, 1.0);
    context.paint().map_err(|err| ExtractError::Render(err.to_string()))?;
    context.scale(scale, scale);
    page.render(&context);
    surface.flush();
    let mut png = Vec::new();
    surface
        .write_to_png(&mut png)
        .map_err(|err| ExtractError::Render(err.to_string()))?;
    Ok(png)
}

fn inspect_epub(path: &Path) -> Result<EpubDetails, ExtractError> {
    let file = File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let container = read_zip_string(&mut archive, "META-INF/container.xml")?;
    let opf_path = rootfile_path(&container)?;
    let opf = read_zip_string(&mut archive, &opf_path)?;
    let document = Document::parse(&opf).map_err(|err| ExtractError::Xml(err.to_string()))?;
    let title = text_named(&document, "title").unwrap_or_else(|| fallback_title(path));
    let author = text_named(&document, "creator").unwrap_or_default();
    let year = text_named(&document, "date").as_deref().and_then(year_in_text);
    let cover = cover_bytes(&mut archive, &opf_path, &document)?;
    Ok(EpubDetails {
        title,
        author,
        year,
        cover,
    })
}

fn cover_bytes(
    archive: &mut zip::ZipArchive<File>,
    opf_path: &str,
    document: &Document<'_>,
) -> Result<Option<Vec<u8>>, ExtractError> {
    let mut candidates = Vec::new();
    if let Some(href) = manifest_cover_href(document) {
        candidates.push(href);
    }
    if let Some(href) = guide_cover_href(document) {
        candidates.push(href);
    }
    if let Some(href) = first_image_href(document) {
        candidates.push(href);
    }
    for href in candidates {
        if let Some(bytes) = read_href(archive, opf_path, &href, document)? {
            return Ok(Some(bytes));
        }
    }
    Ok(None)
}

fn read_href(
    archive: &mut zip::ZipArchive<File>,
    opf_path: &str,
    href: &str,
    document: &Document<'_>,
) -> Result<Option<Vec<u8>>, ExtractError> {
    let Some(zip_path) = join_zip_path(opf_path, href) else {
        return Ok(None);
    };
    if is_markup(&zip_path) {
        let markup = read_zip_string(archive, &zip_path).unwrap_or_default();
        let Some(src) = first_img_src(&markup) else {
            return Ok(None);
        };
        let Some(image_path) = join_zip_path(&zip_path, &src) else {
            return Ok(None);
        };
        return Ok(read_zip_bytes(archive, &image_path).ok());
    }
    if !is_image_path(&zip_path) && !manifest_says_image(document, href) {
        return Ok(None);
    }
    Ok(read_zip_bytes(archive, &zip_path).ok())
}

fn manifest_cover_href(document: &Document<'_>) -> Option<String> {
    for item in document.descendants().filter(|node| node.tag_name().name() == "item") {
        let properties = item.attribute("properties").unwrap_or("");
        if properties.split_whitespace().any(|token| token == "cover-image") {
            return item.attribute("href").map(str::to_string);
        }
    }
    let cover_id = document.descendants().find_map(|node| {
        if node.tag_name().name() != "meta" {
            return None;
        }
        let name = node.attribute("name")?;
        if !name.eq_ignore_ascii_case("cover") {
            return None;
        }
        node.attribute("content").map(str::to_string)
    })?;
    manifest_href_by_id(document, &cover_id)
}

fn guide_cover_href(document: &Document<'_>) -> Option<String> {
    document.descendants().find_map(|node| {
        if node.tag_name().name() != "reference" {
            return None;
        }
        let kind = node.attribute("type")?;
        if !kind.eq_ignore_ascii_case("cover") {
            return None;
        }
        node.attribute("href").map(str::to_string)
    })
}

fn first_image_href(document: &Document<'_>) -> Option<String> {
    document.descendants().find_map(|node| {
        if node.tag_name().name() != "item" {
            return None;
        }
        let href = node.attribute("href")?;
        let media = node.attribute("media-type").unwrap_or("");
        if media.starts_with("image/") || is_image_path(href) {
            Some(href.to_string())
        } else {
            None
        }
    })
}

fn manifest_href_by_id(document: &Document<'_>, id: &str) -> Option<String> {
    document.descendants().find_map(|node| {
        if node.tag_name().name() == "item" && node.attribute("id") == Some(id) {
            node.attribute("href").map(str::to_string)
        } else {
            None
        }
    })
}

fn manifest_says_image(document: &Document<'_>, href: &str) -> bool {
    document.descendants().any(|node| {
        node.tag_name().name() == "item"
            && node.attribute("href") == Some(href)
            && node
                .attribute("media-type")
                .unwrap_or("")
                .starts_with("image/")
    })
}

fn text_named(document: &Document<'_>, name: &str) -> Option<String> {
    document.descendants().find_map(|node| {
        if node.tag_name().name() != name {
            return None;
        }
        let text = node.text()?.trim();
        if text.is_empty() { None } else { Some(text.to_string()) }
    })
}

fn rootfile_path(container_xml: &str) -> Result<String, ExtractError> {
    let document = Document::parse(container_xml).map_err(|err| ExtractError::Xml(err.to_string()))?;
    document
        .descendants()
        .find_map(|node| {
            if node.tag_name().name() == "rootfile" {
                node.attribute("full-path").map(str::to_string)
            } else {
                None
            }
        })
        .ok_or_else(|| ExtractError::Xml("EPUB container has no root file".to_string()))
}

fn read_zip_string<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<String, ExtractError> {
    let bytes = read_zip_bytes(archive, name)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn read_zip_bytes<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>, ExtractError> {
    let mut entry = archive.by_name(name)?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn join_zip_path(base_file: &str, href: &str) -> Option<String> {
    let href = percent_decode(href);
    let href = href.split('#').next().unwrap_or("").trim();
    if href.is_empty() || href.contains(':') {
        return None;
    }
    let base_dir = Path::new(base_file).parent().unwrap_or(Path::new(""));
    let joined = base_dir.join(href);
    let mut parts = Vec::new();
    for component in joined.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(
                std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or(""),
                16,
            ) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn first_img_src(markup: &str) -> Option<String> {
    let lower = markup.to_ascii_lowercase();
    let start = lower.find("<img")?;
    let rest = &markup[start..];
    let end = rest.find('>').unwrap_or(rest.len());
    let tag = &rest[..end];
    attribute_from_tag(tag, "src")
}

fn attribute_from_tag(tag: &str, name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let needle = format!("{name}={quote}");
        if let Some(position) = tag.find(&needle) {
            let value_start = position + needle.len();
            let value_end = tag[value_start..].find(quote)? + value_start;
            return Some(tag[value_start..value_end].to_string());
        }
    }
    None
}

fn is_markup(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".xhtml")
        || lower.ends_with(".html")
        || lower.ends_with(".htm")
        || lower.ends_with(".xml")
}

fn is_image_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".png")
        || lower.ends_with(".gif")
        || lower.ends_with(".webp")
}

fn clean_meta(value: Option<&str>) -> Option<String> {
    let text = value?.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn fallback_title(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("Untitled")
        .to_string()
}

fn year_in_text(value: &str) -> Option<i32> {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index + 3 < bytes.len() {
        if bytes[index..index + 4].iter().all(u8::is_ascii_digit) {
            let year: i32 = value[index..index + 4].parse().ok()?;
            if (1000..=9999).contains(&year) {
                return Some(year);
            }
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    #[test]
    fn format_labels() {
        assert_eq!(BookFormat::Pdf.label(), "PDF");
        assert_eq!(BookFormat::Epub.label(), "EPUB");
    }

    fn epub_with(opf: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ebook-reader-epub-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("book.epub");
        let file = File::create(&path).unwrap();
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        writer
            .start_file("META-INF/container.xml", options)
            .unwrap();
        writer
            .write_all(
                br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
            )
            .unwrap();
        writer.start_file("OEBPS/content.opf", options).unwrap();
        writer.write_all(opf.as_bytes()).unwrap();
        for (name, bytes) in files {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
        path
    }

    #[test]
    fn reads_epub3_cover_and_metadata() {
        let png = tiny_png();
        let opf = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>The Left Hand</dc:title>
    <dc:creator>Ursula K. Le Guin</dc:creator>
    <dc:date>1969-03-01</dc:date>
  </metadata>
  <manifest>
    <item id="cover" href="images/cover.png" media-type="image/png" properties="cover-image"/>
  </manifest>
</package>"#;
        let path = epub_with(opf, &[("OEBPS/images/cover.png", png.as_slice())]);
        let details = inspect_epub(&path).unwrap();
        assert_eq!(details.title, "The Left Hand");
        assert_eq!(details.author, "Ursula K. Le Guin");
        assert_eq!(details.year, Some(1969));
        assert_eq!(details.cover.unwrap(), png);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn reads_epub2_cover_and_rejects_parent_paths() {
        let png = tiny_png();
        let opf = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Walkaway</dc:title>
    <dc:creator>Cory Doctorow</dc:creator>
    <meta name="cover" content="cover-id"/>
  </metadata>
  <manifest>
    <item id="cover-id" href="../secret.png" media-type="image/png"/>
    <item id="page" href="chapter.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <guide>
    <reference type="cover" href="cover.xhtml"/>
  </guide>
</package>"#;
        let xhtml = br#"<html><body><img src="images/front.png"/></body></html>"#;
        let path = epub_with(
            opf,
            &[
                ("OEBPS/cover.xhtml", xhtml.as_slice()),
                ("OEBPS/images/front.png", png.as_slice()),
            ],
        );
        let details = inspect_epub(&path).unwrap();
        assert_eq!(details.cover.unwrap(), png);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn renders_pdf_first_page() {
        let dir = std::env::temp_dir().join(format!(
            "ebook-reader-pdf-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hello.pdf");
        std::fs::write(&path, minimal_pdf()).unwrap();
        let extracted = extract(&path).unwrap();
        assert_eq!(extracted.format, BookFormat::Pdf);
        assert_eq!(extracted.title, "hello");
        assert!(extracted.cover_png.starts_with(b"\x89PNG"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn title_card_is_png() {
        let png = title_card_png("A Long Title That Wraps", "Author").unwrap();
        assert!(png.starts_with(b"\x89PNG"));
    }

    fn minimal_pdf() -> Vec<u8> {
        let objects = [
            b"1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n".to_vec(),
            b"2 0 obj<</Type/Pages/Count 1/Kids[3 0 R]>>endobj\n".to_vec(),
            b"3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 300 400]/Contents 4 0 R/Resources<</Font<</F1 5 0 R>>>>>>endobj\n".to_vec(),
            b"4 0 obj<</Length 44>>stream\nBT /F1 24 Tf 40 200 Td (Hello) Tj ET\nendstream\nendobj\n".to_vec(),
            b"5 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>endobj\n".to_vec(),
        ];
        let mut body = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for object in &objects {
            offsets.push(body.len());
            body.extend_from_slice(object);
        }
        let xref_at = body.len();
        let mut xref = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
        for offset in offsets {
            xref.push_str(&format!("{offset:010} 00000 n \n"));
        }
        xref.push_str(&format!(
            "trailer<</Size {}/Root 1 0 R>>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        ));
        body.extend(xref.as_bytes());
        body
    }

    fn tiny_png() -> Vec<u8> {
        let bytes = [
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
            0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
            0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63, 0xf8,
            0xcf, 0xc0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x00, 0x05, 0xfe, 0x02, 0xfe, 0x00, 0x00,
            0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        bytes.to_vec()
    }

    #[test]
    fn join_zip_path_decodes_and_blocks_escape() {
        assert_eq!(
            join_zip_path("OEBPS/content.opf", "images/cover%20art.png").as_deref(),
            Some("OEBPS/images/cover art.png")
        );
        assert!(join_zip_path("OEBPS/content.opf", "../secret.png").is_none());
    }

}
