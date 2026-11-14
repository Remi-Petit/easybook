//! Réécriture des URL d'un document de livre.
//!
//! Un chapitre d'EPUB est écrit pour être lu **dans son archive** : ses images,
//! ses feuilles de style et ses polices y sont désignés en relatif
//! (`../Images/p1.jpg`). Une fois le chapitre injecté dans la page de
//! l'interface, ces chemins ne veulent plus rien dire — ils viseraient
//! l'interface elle-même.
//!
//! On les remplace donc, côté serveur, par des URL absolues vers l'API
//! (`/api/asset?path=<livre>&inner=<ressource>`). Les feuilles de style sont
//! réécrites de la même façon (`url(...)`, `@import`), sans quoi leurs images et
//! leurs polices retomberaient sur l'origine de l'interface.

use regex::Regex;
use std::sync::OnceLock;

/// URL d'une ressource d'un livre, servie par l'API.
pub fn asset_url(asset_base: &str, book_rel: &str, inner: &str) -> String {
    format!(
        "{}/api/asset?path={}&inner={}",
        asset_base.trim_end_matches('/'),
        encode(book_rel),
        encode(inner)
    )
}

/// Encode une valeur pour un paramètre de requête.
pub fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Décode les séquences `%XX` (les `href` d'un EPUB sont des URL).
pub fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(high * 16 + low);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Dossier contenant un chemin d'archive (`""` s'il n'y en a pas).
pub fn dir_of(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some((dir, _)) => dir,
        None => "",
    }
}

/// Résout `href` depuis le dossier `dir`, en repliant `.` et `..`.
pub fn join_path(dir: &str, href: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for segment in dir.split('/').chain(href.split('/')) {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            segment => parts.push(segment),
        }
    }
    parts.join("/")
}

/// `true` pour une URL absolue (`http:`, `data:`…), un protocole relatif
/// (`//cdn`) ou un fragment (`#note1`) : rien à résoudre.
fn is_absolute(value: &str) -> bool {
    let value = value.trim();
    if value.starts_with('#') || value.starts_with("//") {
        return true;
    }
    match value.split_once(':') {
        Some((scheme, _)) => {
            !scheme.is_empty()
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        }
        None => false,
    }
}

/// Chemin dans l'archive désigné par `value`, ou `None` s'il n'y a rien à
/// résoudre (URL absolue, fragment, valeur vide).
fn inner_for(dir: &str, value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || is_absolute(value) {
        return None;
    }
    // Un fragment désigne un endroit du document, pas un fichier.
    let path = value.split('#').next().unwrap_or("");
    if path.is_empty() {
        return None;
    }
    Some(join_path(dir, &decode(path)))
}

/// Réécrit les URL d'un document XHTML/HTML.
pub fn html(html: &str, doc_dir: &str, book_rel: &str, asset_base: &str) -> String {
    // D'abord les blocs `<style>` : ce sont des feuilles de style en ligne.
    let html = style_re()
        .replace_all(html, |caps: &regex::Captures<'_>| {
            format!(
                "<style{}>{}</style>",
                &caps[1],
                css(&caps[2], doc_dir, book_rel, asset_base)
            )
        })
        .into_owned();

    let mut out = String::with_capacity(html.len() + 512);
    let mut rest = html.as_str();

    while let Some(position) = rest.find('<') {
        out.push_str(&rest[..position]);
        rest = &rest[position..];

        // Commentaire : recopié tel quel (il peut contenir un `>`).
        if rest.starts_with("<!--") {
            match rest[4..].find("-->") {
                Some(index) => {
                    let end = 4 + index + 3;
                    out.push_str(&rest[..end]);
                    rest = &rest[end..];
                }
                None => {
                    out.push_str(rest);
                    rest = "";
                }
            }
            continue;
        }

        let Some(end) = tag_end(rest) else {
            out.push_str(rest);
            rest = "";
            break;
        };
        out.push_str(&rewrite_tag(&rest[..=end], doc_dir, book_rel, asset_base));
        rest = &rest[end + 1..];
    }

    out.push_str(rest);
    out
}

/// Index du `>` qui ferme la balise commençant à `s`, hors guillemets.
fn tag_end(s: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (index, c) in s.char_indices() {
        match (quote, c) {
            (None, '"') | (None, '\'') => quote = Some(c),
            (Some(open), c) if open == c => quote = None,
            (None, '>') => return Some(index),
            _ => {}
        }
    }
    None
}

/// Réécrit une balise complète (`<img src="…">`).
fn rewrite_tag(tag: &str, doc_dir: &str, book_rel: &str, asset_base: &str) -> String {
    // Fermetures, commentaires et déclarations : rien à toucher.
    if tag.starts_with("</") || tag.starts_with("<!") || tag.starts_with("<?") {
        return tag.to_string();
    }

    let name: String = tag[1..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '-' | '_'))
        .collect::<String>()
        .to_ascii_lowercase();
    let is_anchor = name == "a";

    attr_re()
        .replace_all(tag, |caps: &regex::Captures<'_>| {
            let attr = &caps[1];
            let lower = attr.to_ascii_lowercase();
            let value = caps
                .get(3)
                .or_else(|| caps.get(4))
                .map(|m| m.as_str())
                .unwrap_or("");

            // Un gestionnaire d'événements (`onerror=`, `onclick=` …) n'a rien à
            // faire dans un livre : le contenu vient d'un fichier tiers.
            if lower.starts_with("on")
                && lower.len() > 2
                && lower[2..].chars().all(|c| c.is_ascii_alphabetic())
            {
                return String::new();
            }

            if !matches!(lower.as_str(), "src" | "href" | "xlink:href" | "poster") {
                return caps[0].to_string();
            }

            // Lien d'ancre : la cible est un autre document de l'archive. La
            // navigation se fait par la liste des chapitres ; un lien suivi ici
            // ferait quitter le lecteur. On garde les fragments internes
            // (`#note1`) et les liens externes.
            if is_anchor {
                if is_absolute(value) {
                    return caps[0].to_string();
                }
                return String::new();
            }

            match inner_for(doc_dir, value) {
                Some(inner) => format!(r#"{}="{}""#, attr, asset_url(asset_base, book_rel, &inner)),
                None => caps[0].to_string(),
            }
        })
        .into_owned()
}

/// Réécrit une feuille de style.
pub fn css(text: &str, sheet_dir: &str, book_rel: &str, asset_base: &str) -> String {
    let with_urls = url_re()
        .replace_all(text, |caps: &regex::Captures<'_>| {
            // `url(x)`, `url("x")` ou `url('x')` : trois écritures, une valeur.
            let value = caps
                .get(1)
                .or_else(|| caps.get(2))
                .or_else(|| caps.get(3))
                .map(|m| m.as_str())
                .unwrap_or("");
            match inner_for(sheet_dir, value) {
                Some(inner) => {
                    format!(r#"url("{}")"#, asset_url(asset_base, book_rel, &inner))
                }
                None => caps[0].to_string(),
            }
        })
        .into_owned();

    import_re()
        .replace_all(&with_urls, |caps: &regex::Captures<'_>| {
            match inner_for(sheet_dir, &caps[2]) {
                Some(inner) => format!(r#"@import "{}""#, asset_url(asset_base, book_rel, &inner)),
                None => caps[0].to_string(),
            }
        })
        .into_owned()
}

fn attr_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)([a-zA-Z_:][-a-zA-Z0-9_:.]*)\s*=\s*("([^"]*)"|'([^']*)')"#).unwrap()
    })
}

fn style_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?is)<style([^>]*)>(.*?)</style>"#).unwrap())
}

fn url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)url\(\s*(?:"([^"]*)"|'([^']*)'|([^)'"\s]+))\s*\)"#).unwrap()
    })
}

fn import_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Pas de rétroréférence (le crate `regex` n'en a pas) : les deux guillemets
    // sont acceptés indépendamment, ce qui est sans conséquence en CSS.
    RE.get_or_init(|| Regex::new(r#"(?i)@import\s+(["'])([^"']+)["']"#).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "http://127.0.0.1:8091";

    #[test]
    fn resolves_relative_paths() {
        assert_eq!(
            join_path("OEBPS/Text", "../Images/p1.jpg"),
            "OEBPS/Images/p1.jpg"
        );
        assert_eq!(
            join_path("OEBPS/Text", "chap2.xhtml"),
            "OEBPS/Text/chap2.xhtml"
        );
        assert_eq!(join_path("", "cover.jpg"), "cover.jpg");
        assert_eq!(dir_of("OEBPS/Text/chap1.xhtml"), "OEBPS/Text");
        assert_eq!(dir_of("cover.jpg"), "");
    }

    #[test]
    fn encodes_and_decodes_query_values() {
        assert_eq!(encode("Chainsaw Man/T01.epub"), "Chainsaw%20Man%2FT01.epub");
        assert_eq!(decode("Chainsaw%20Man%2FT01.epub"), "Chainsaw Man/T01.epub");
    }

    #[test]
    fn rewrites_images_and_stylesheets() {
        let source = r#"<html><head><link rel="stylesheet" href="style.css"/></head>
            <body><img src="../Images/p1.jpg"/><a href="chap2.xhtml">suite</a></body></html>"#;
        let out = html(source, "OEBPS/Text", "Livre.epub", BASE);

        assert!(out.contains(&format!(
            r#"href="{BASE}/api/asset?path=Livre.epub&inner=OEBPS%2FText%2Fstyle.css""#
        )));
        assert!(out.contains(&format!(
            r#"src="{BASE}/api/asset?path=Livre.epub&inner=OEBPS%2FImages%2Fp1.jpg""#
        )));
        // Lien interne neutralisé, texte conservé.
        assert!(out.contains(">suite</a>"));
        assert!(!out.contains("chap2.xhtml"));
    }

    #[test]
    fn keeps_external_links_and_fragments_but_drops_handlers() {
        let source = r##"<a href="https://example.org">web</a><a href="#note">note</a>
            <img src="p.jpg" onerror="alert(1)"/>"##;
        let out = html(source, "Text", "b.epub", BASE);
        assert!(out.contains(r#"href="https://example.org""#));
        assert!(out.contains(r##"href="#note""##));
        assert!(!out.contains("onerror"));
    }

    #[test]
    fn rewrites_css_urls_and_imports() {
        let sheet = r#"@import "base.css";
            body { background: url('../Images/bg.png'); }
            .glyph { src: url(data:font/woff2;base64,AAAA) format("woff2"); }"#;
        let out = css(sheet, "OEBPS/Styles", "Livre.epub", BASE);

        assert!(out.contains(&format!(
            r#"@import "{BASE}/api/asset?path=Livre.epub&inner=OEBPS%2FStyles%2Fbase.css""#
        )));
        assert!(out.contains(&format!(
            r#"url("{BASE}/api/asset?path=Livre.epub&inner=OEBPS%2FImages%2Fbg.png")"#
        )));
        // `data:` n'est pas un chemin d'archive.
        assert!(out.contains("data:font/woff2;base64,AAAA"));
    }
}
