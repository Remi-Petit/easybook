//! Lecture d'un EPUB : sommaire (`spine` + table des matières) et chapitres.
//!
//! Un EPUB est une archive ZIP : `META-INF/container.xml` désigne le `.opf`, qui
//! décrit les métadonnées, le manifeste (fichiers du livre) et le `spine`
//! (l'ordre de lecture). La table des matières vient soit du `toc.ncx` (EPUB 2),
//! soit d'un document de navigation XHTML (EPUB 3) — on essaie les deux.

use crate::archive;
use crate::rewrite;
use roxmltree::{Document, Node};
use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use zip::ZipArchive;

use super::filecache::FileCache;
use super::{BookFormat, Chapter, Outline, Problem};

/// Format EPUB, tel que l'enregistre [`super::module`].
pub struct Epub;

/// Instance unique du format (voir [`super::module`]).
pub static EPUB: Epub = Epub;

impl BookFormat for Epub {
    fn id(&self) -> &'static str {
        "epub"
    }

    fn outline(&self, path: &Path) -> Result<Option<Outline>, Problem> {
        Ok(Some((*outline(path)?).clone()))
    }

    /// Nombre de **chapitres** : le texte d'un EPUB se recompose à l'écran, il
    /// n'a pas de pages fixes. C'est le `spine` qui fixe les documents du livre.
    fn pages(&self, path: &Path) -> Result<Option<usize>, Problem> {
        Ok(Some(outline(path)?.chapters.len()))
    }

    fn cover(&self, path: &Path) -> Option<Vec<u8>> {
        cover(path)
    }

    fn asset(&self, path: &Path, inner: &str) -> Option<Vec<u8>> {
        entry(path, inner)
    }

    fn chapter(
        &self,
        path: &Path,
        index: usize,
        book_rel: &str,
        asset_base: &str,
    ) -> Result<String, Problem> {
        chapter(path, index, book_rel, asset_base)
    }
}

/// Sommaires gardés : le lire (container, OPF, table des matières) coûte plus
/// que le chapitre demandé, et serait refait à chaque chapitre tourné.
static OUTLINES: FileCache<Outline> = FileCache::new(32);

/// Sommaire d'un EPUB.
fn outline(path: &Path) -> Result<Arc<Outline>, Problem> {
    OUTLINES.get_or_load(path, || {
        let mut zip = archive::open(path)?;
        outline_from(&mut zip)
    })
}

/// Contenu brut d'une ressource de l'archive.
fn entry(path: &Path, inner: &str) -> Option<Vec<u8>> {
    let mut zip = archive::open(path).ok()?;
    archive::read_named(&mut zip, inner)
}

/// Image de couverture déclarée par le livre, si elle existe.
///
/// Trois écritures coexistent, dans cet ordre de fiabilité :
/// 1. EPUB 3 — l'élément du manifeste porte `properties="cover-image"` ;
/// 2. EPUB 2 — `<meta name="cover" content="id"/>` désigne un élément du
///    manifeste (parfois le nom de fichier, rarement l'identifiant exact) ;
/// 3. repli — la première image dont le chemin évoque une couverture.
///
/// La couverture n'est pas lue au scan : elle coûte l'ouverture de l'archive,
/// et seuls les livres affichés en ont besoin.
fn cover(path: &Path) -> Option<Vec<u8>> {
    let mut zip = archive::open(path).ok()?;
    cover_from(&mut zip)
}

fn cover_from(zip: &mut ZipArchive<File>) -> Option<Vec<u8>> {
    let opf_path = container_opf(zip)?;
    let text = read_xml(zip, &opf_path)?;
    let opf = Document::parse(&text).ok()?;
    let opf_dir = rewrite::dir_of(&opf_path).to_string();
    let manifest = manifest(&opf, &opf_dir);

    // 1. EPUB 3 : c'est le manifeste qui le dit.
    if let Some(item) = manifest.values().find(|item| {
        item.properties
            .split_whitespace()
            .any(|p| p == "cover-image")
    }) && let Some(bytes) = archive::read_named(zip, &item.href)
    {
        return Some(bytes);
    }

    // 2. EPUB 2 : `<meta name="cover" content="…"/>`. La valeur est en général
    //    l'identifiant d'un élément du manifeste ; certains producteurs y mettent
    //    directement le nom du fichier, d'où le second essai.
    let declared = opf
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "meta")
        .find(|node| {
            node.attribute("name")
                .is_some_and(|name| name.eq_ignore_ascii_case("cover"))
        })
        .and_then(|node| node.attribute("content"));

    if let Some(declared) = declared {
        let by_id = manifest.get(declared);
        let by_path = manifest.values().find(|item| {
            item.href.eq_ignore_ascii_case(declared) || item.href.ends_with(&format!("/{declared}"))
        });
        if let Some(item) = by_id.or(by_path)
            && let Some(bytes) = archive::read_named(zip, &item.href)
        {
            return Some(bytes);
        }
    }

    // 3. Repli : une image dont le nom parle de couverture (`cover.jpg`,
    //    `Cover2.png`…). Sinon, la première image du manifeste : mieux que rien
    //    pour un livre qui n'a rien déclaré.
    let candidate = manifest
        .values()
        .filter(|item| item.media_type.starts_with("image/"))
        .find(|item| item.href.to_ascii_lowercase().contains("cover"))
        .or_else(|| {
            manifest
                .values()
                .find(|item| item.media_type.starts_with("image/"))
        })?;

    archive::read_named(zip, &candidate.href)
}

/// Entrées du manifeste, indexées par identifiant.
fn manifest(opf: &Document<'_>, opf_dir: &str) -> HashMap<String, Item> {
    let mut manifest: HashMap<String, Item> = HashMap::new();
    for node in opf
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "item")
    {
        let (Some(id), Some(href)) = (node.attribute("id"), node.attribute("href")) else {
            continue;
        };
        manifest.insert(
            id.to_string(),
            Item {
                href: rewrite::join_path(opf_dir, &rewrite::decode(href)),
                media_type: node.attribute("media-type").unwrap_or("").to_string(),
                properties: node.attribute("properties").unwrap_or("").to_string(),
            },
        );
    }
    manifest
}

/// Document d'un chapitre, prêt à être affiché.
///
/// Les URL relatives sont réécrites vers `/api/asset` (voir [`crate::rewrite`]) :
/// le document peut donc être injecté tel quel dans la page du lecteur.
fn chapter(path: &Path, index: usize, book_rel: &str, asset_base: &str) -> Result<String, Problem> {
    let mut zip = archive::open(path)?;
    let outline = outline(path)?;
    let chapter = outline.chapters.get(index).ok_or_else(|| {
        Problem::new("epub.chapter-out-of-range")
            .param("index", index.to_string())
            .param("count", outline.chapters.len().to_string())
    })?;

    let bytes = archive::read_named(&mut zip, &chapter.href)
        .ok_or_else(|| Problem::with("epub.chapter-missing", "href", &chapter.href))?;
    let text = String::from_utf8_lossy(&bytes);

    Ok(rewrite::html(
        &text,
        rewrite::dir_of(&chapter.href),
        book_rel,
        asset_base,
    ))
}

/// Description d'une entrée du manifeste.
struct Item {
    /// Chemin dans l'archive (déjà résolu depuis le dossier de l'OPF).
    href: String,
    media_type: String,
    properties: String,
}

fn outline_from(zip: &mut ZipArchive<File>) -> Result<Outline, Problem> {
    // 1. `container.xml` : où est l'OPF ?
    let opf_path = container_opf(zip).ok_or_else(|| Problem::new("epub.container-missing"))?;

    // 2. L'OPF : métadonnées, manifeste, spine. Le document emprunte le texte
    //    (local) : il vit le temps de cette fonction, sans copie ni fuite.
    let bytes =
        archive::read_named(zip, &opf_path).ok_or_else(|| Problem::new("epub.opf-missing"))?;
    let text = String::from_utf8_lossy(&bytes);
    let opf = Document::parse(&text)
        .map_err(|e| Problem::with("epub.opf-invalid", "error", e.to_string()))?;
    let opf_dir = rewrite::dir_of(&opf_path).to_string();

    let title = element_text(&opf, "title");
    let author = element_text(&opf, "creator");
    let manifest = manifest(&opf, &opf_dir);

    let mut chapters: Vec<Chapter> = Vec::new();
    for node in opf
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "itemref")
    {
        // `linear="no"` : annexe (couverture, colophon), hors du fil de lecture.
        if node.attribute("linear") == Some("no") {
            continue;
        }
        let Some(item) = node.attribute("idref").and_then(|id| manifest.get(id)) else {
            continue;
        };
        if !is_document(&item.media_type) {
            continue;
        }
        chapters.push(Chapter {
            index: chapters.len(),
            title: String::new(),
            href: item.href.clone(),
        });
    }

    // 3. La table des matières donne un titre lisible à chaque document.
    let labels = toc_labels(zip, &opf, &manifest);

    for chapter in &mut chapters {
        chapter.title = labels
            .get(&chapter.href)
            .cloned()
            .unwrap_or_else(|| format!("Chapitre {}", chapter.index + 1));
    }

    Ok(Outline {
        title,
        author,
        chapters,
    })
}

/// Le média du `spine` est-il un document affichable ?
fn is_document(media_type: &str) -> bool {
    matches!(
        media_type,
        "application/xhtml+xml" | "text/html" | "image/svg+xml"
    )
}

/// Libellés de la table des matières, indexés par chemin d'archive.
///
/// EPUB 3 d'abord (document de navigation), EPUB 2 ensuite (`toc.ncx`) : les
/// deux peuvent coexister, et le premier libellé trouvé l'emporte — c'est celui
/// du niveau le plus haut.
fn toc_labels(
    zip: &mut ZipArchive<File>,
    opf: &Document<'_>,
    manifest: &HashMap<String, Item>,
) -> HashMap<String, String> {
    let mut labels = HashMap::new();

    // EPUB 3 : l'élément du manifeste porte `properties="nav"`.
    if let Some(nav) = manifest
        .values()
        .find(|item| item.properties.split_whitespace().any(|p| p == "nav"))
        && let Some(text) = read_xml(zip, &nav.href)
        && let Ok(document) = Document::parse(&text)
    {
        let nav_dir = rewrite::dir_of(&nav.href).to_string();
        let toc = document
            .descendants()
            .find(|node| node.is_element() && node.tag_name().name() == "nav" && is_toc(node));

        if let Some(toc) = toc {
            for link in toc
                .descendants()
                .filter(|node| node.is_element() && node.tag_name().name() == "a")
            {
                let Some(href) = link.attribute("href") else {
                    continue;
                };
                let label = text_of(&link);
                if label.is_empty() {
                    continue;
                }
                labels.entry(target_of(&nav_dir, href)).or_insert(label);
            }
        }
    }

    // EPUB 2 : `toc.ncx`, désigné par le manifeste ou par `spine@toc`.
    let ncx = manifest
        .values()
        .find(|item| item.media_type == "application/x-dtbncx+xml")
        .map(|item| item.href.clone())
        .or_else(|| {
            opf.descendants()
                .find(|node| node.is_element() && node.tag_name().name() == "spine")
                .and_then(|spine| spine.attribute("toc"))
                .and_then(|id| manifest.get(id))
                .map(|item| item.href.clone())
        });

    if let Some(ncx) = ncx
        && let Some(text) = read_xml(zip, &ncx)
        && let Ok(document) = Document::parse(&text)
    {
        let ncx_dir = rewrite::dir_of(&ncx).to_string();
        for point in document
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == "navPoint")
        {
            let Some(source) = point
                .descendants()
                .find(|node| node.is_element() && node.tag_name().name() == "content")
                .and_then(|node| node.attribute("src"))
            else {
                continue;
            };
            let label = point
                .descendants()
                .find(|node| node.is_element() && node.tag_name().name() == "navLabel")
                .map(|label| text_of(&label))
                .unwrap_or_default();
            if label.is_empty() {
                continue;
            }
            labels.entry(target_of(&ncx_dir, source)).or_insert(label);
        }
    }

    labels
}

/// Cible d'un lien de sommaire : chemin d'archive absolu, sans fragment.
fn target_of(dir: &str, href: &str) -> String {
    let path = href.split('#').next().unwrap_or("");
    rewrite::join_path(dir, &rewrite::decode(path))
}

/// `true` si un `<nav>` est la table des matières (`epub:type="toc"`).
fn is_toc(node: &Node<'_, '_>) -> bool {
    node.attributes()
        .any(|attr| attr.name() == "type" && attr.value().split_whitespace().any(|v| v == "toc"))
}

/// Texte d'un élément, espaces normalisés.
fn text_of(node: &Node<'_, '_>) -> String {
    let mut out = String::new();
    for descendant in node.descendants().filter(|n| n.is_text()) {
        if let Some(text) = descendant.text() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(text);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Texte du premier élément de ce nom dans le document.
fn element_text(document: &Document<'_>, name: &str) -> Option<String> {
    document
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == name)
        .map(|node| text_of(&node))
        .filter(|text| !text.is_empty())
}

/// Chemin de l'OPF, lu dans `META-INF/container.xml`.
fn container_opf(zip: &mut ZipArchive<File>) -> Option<String> {
    let bytes = archive::read_named(zip, "META-INF/container.xml")?;
    let text = String::from_utf8_lossy(&bytes);
    let document = Document::parse(&text).ok()?;
    document
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "rootfile")
        .and_then(|node| node.attribute("full-path"))
        .map(rewrite::decode)
}

/// Lit une entrée XML et rend son texte, que l'appelant analyse.
///
/// Un `Document` emprunte le texte qu'il analyse : le garder au-delà de la
/// fonction demanderait une structure auto-référencée (ou une fuite). Chaque
/// appelant analyse donc son document dans sa propre portée.
fn read_xml(zip: &mut ZipArchive<File>, name: &str) -> Option<String> {
    archive::read_named(zip, name).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    /// EPUB minimal : OPF + navigation + deux chapitres + une image.
    fn epub() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Test.epub");
        let file = File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);

        let entries: Vec<(&str, &str)> = vec![
            (
                "META-INF/container.xml",
                r#"<?xml version="1.0"?>
                <container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
                  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
                </container>"#,
            ),
            (
                "OEBPS/content.opf",
                r#"<?xml version="1.0"?>
                <package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">
                  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
                    <dc:title>Le Livre</dc:title>
                    <dc:creator>Autrice</dc:creator>
                  </metadata>
                  <manifest>
                    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
                    <item id="c1" href="Text/chap1.xhtml" media-type="application/xhtml+xml"/>
                    <item id="c2" href="Text/chap2.xhtml" media-type="application/xhtml+xml"/>
                    <item id="img" href="Images/p1.jpg" media-type="image/jpeg"/>
                  </manifest>
                  <spine>
                    <itemref idref="c1"/>
                    <itemref idref="c2"/>
                  </spine>
                </package>"#,
            ),
            (
                "OEBPS/nav.xhtml",
                r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
                  <body><nav epub:type="toc"><ol>
                    <li><a href="Text/chap1.xhtml">Début</a></li>
                    <li><a href="Text/chap2.xhtml">Suite</a></li>
                  </ol></nav></body></html>"#,
            ),
            (
                "OEBPS/Text/chap1.xhtml",
                r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>1</title></head>
                   <body><h1>Un</h1><img src="../Images/p1.jpg"/></body></html>"#,
            ),
            (
                "OEBPS/Text/chap2.xhtml",
                r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Deux</h1></body></html>"#,
            ),
            ("OEBPS/Images/p1.jpg", "jpeg"),
        ];

        for (name, content) in entries {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        (dir, path)
    }

    #[test]
    fn reads_metadata_and_spine_with_toc_labels() {
        let (_dir, path) = epub();
        let outline = outline(&path).unwrap();

        assert_eq!(outline.title.as_deref(), Some("Le Livre"));
        assert_eq!(outline.author.as_deref(), Some("Autrice"));
        assert_eq!(outline.chapters.len(), 2);
        assert_eq!(outline.chapters[0].title, "Début");
        assert_eq!(outline.chapters[1].title, "Suite");
        assert_eq!(outline.chapters[0].href, "OEBPS/Text/chap1.xhtml");
    }

    #[test]
    fn chapter_html_points_images_at_the_api() {
        let (_dir, path) = epub();
        let html = chapter(&path, 0, "Test.epub", "http://127.0.0.1:8091").unwrap();
        assert!(html.contains("<h1>Un</h1>"));
        assert!(html.contains(
            "http://127.0.0.1:8091/api/asset?path=Test.epub&inner=OEBPS%2FImages%2Fp1.jpg"
        ));
    }

    #[test]
    fn chapter_out_of_range_is_an_error() {
        let (_dir, path) = epub();
        assert!(chapter(&path, 5, "Test.epub", "http://x").is_err());
    }

    /// EPUB **2** : `toc.ncx` au lieu d'un document de navigation, et la
    /// couverture déclarée par `<meta name="cover">`. C'est le format de la
    /// majorité des livres déjà distribués.
    fn epub2() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Ancien.epub");
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());

        let entries: Vec<(&str, &str)> = vec![
            (
                "META-INF/container.xml",
                r#"<?xml version="1.0"?>
                <container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
                  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
                </container>"#,
            ),
            (
                "OEBPS/content.opf",
                r#"<?xml version="1.0"?>
                <package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="id">
                  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
                    <dc:title>Livre Deux</dc:title>
                    <dc:creator>Autrice</dc:creator>
                    <meta name="cover" content="cover-img"/>
                  </metadata>
                  <manifest>
                    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
                    <item id="cover-img" href="Images/cover.png" media-type="image/png"/>
                    <item id="c1" href="Text/chap1.xhtml" media-type="application/xhtml+xml"/>
                  </manifest>
                  <spine toc="ncx"><itemref idref="c1"/></spine>
                </package>"#,
            ),
            (
                "OEBPS/toc.ncx",
                r#"<?xml version="1.0"?>
                <ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
                  <navMap>
                    <navPoint id="n1" playOrder="1">
                      <navLabel><text>Le début</text></navLabel>
                      <content src="Text/chap1.xhtml"/>
                    </navPoint>
                  </navMap>
                </ncx>"#,
            ),
            (
                "OEBPS/Text/chap1.xhtml",
                r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Un</h1></body></html>"#,
            ),
            ("OEBPS/Images/cover.png", "couverture"),
        ];

        for (name, content) in entries {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        (dir, path)
    }

    #[test]
    fn reads_an_epub2_toc_and_declared_cover() {
        let (_dir, path) = epub2();

        let outline = outline(&path).unwrap();
        assert_eq!(outline.title.as_deref(), Some("Livre Deux"));
        assert_eq!(outline.chapters.len(), 1);
        // Le libellé vient du `toc.ncx`, pas d'un repli « Chapitre 1 ».
        assert_eq!(outline.chapters[0].title, "Le début");

        assert_eq!(cover(&path).as_deref(), Some(&b"couverture"[..]));
    }

    #[test]
    fn an_undeclared_cover_falls_back_to_the_first_image() {
        // Le livre de test n'a ni `cover-image` ni `<meta name="cover">`.
        let (_dir, path) = epub();
        assert_eq!(cover(&path).as_deref(), Some(&b"jpeg"[..]));
    }

    #[test]
    fn the_page_count_is_the_number_of_spine_documents() {
        let (_dir, path) = epub();
        let pages = BookFormat::pages(&EPUB, &path).unwrap();
        assert_eq!(pages, Some(2));
    }
}
