//! Diagnostic destiné à l'interface : un **code** stable et ses paramètres.
//!
//! Le serveur ignore la langue de la page : il n'envoie donc pas de phrase mais
//! un code que le frontend traduit (`problem.<code>` dans les six langues). Le
//! texte technique d'une bibliothèque (`zip`, `unrar`, `lopdf`) ou du système
//! reste tel quel dans un paramètre : il n'est pas traduisible, mais il est
//! toujours montré, entre les mots traduits de la phrase qui l'entoure.
//!
//! Un code est une constante du binaire : une faute de frappe est refusée à la
//! compilation, et une interface plus ancienne que le serveur retombe sur le
//! détail brut plutôt que sur un vide.

use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;

/// Code à montrer quand un fichier n'a pas de pages (archive vide de planches,
/// document d'un format que le serveur ne sait pas ouvrir…).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    /// Identifiant du message (`comic.no-pages.book`, `epub.opf-missing`…).
    pub code: &'static str,
    /// Valeurs des `{paramètres}` de la phrase traduite.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<&'static str, String>,
}

impl Problem {
    /// Un code seul, sans paramètre.
    pub fn new(code: &'static str) -> Self {
        Self {
            code,
            params: BTreeMap::new(),
        }
    }

    /// Un code et son premier paramètre — le cas courant.
    pub fn with(code: &'static str, key: &'static str, value: impl Into<String>) -> Self {
        Self::new(code).param(key, value)
    }

    /// Ajoute un paramètre (`{key}` dans la phrase traduite).
    pub fn param(mut self, key: &'static str, value: impl Into<String>) -> Self {
        self.params.insert(key, value.into());
        self
    }

    /// Même diagnostic, sans un préfixe de chemin dans ses paramètres.
    ///
    /// Les erreurs d'entrée-sortie nomment le fichier (`<chemin> : <raison>`) :
    /// l'appelant sait déjà de quel livre il parle, et le chemin absolu du
    /// serveur n'a rien à faire dans une fiche.
    pub fn strip_path_prefix(mut self, prefix: &str) -> Self {
        for value in self.params.values_mut() {
            if let Some(rest) = value.strip_prefix(prefix) {
                *value = rest.to_string();
            }
        }
        self
    }
}

/// Forme lisible par un humain et par les journaux : `code clé=valeur`.
impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code)?;
        for (key, value) in &self.params {
            write!(f, " {key}={value}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_are_kept_in_a_stable_order() {
        let problem = Problem::with("a.b.c", "name", "x")
            .param("count", "2")
            .param("name", "y");
        assert_eq!(problem.params.get("name").map(String::as_str), Some("y"));
        assert_eq!(
            serde_json::to_string(&problem).unwrap(),
            r#"{"code":"a.b.c","params":{"count":"2","name":"y"}}"#
        );
    }

    #[test]
    fn a_code_alone_has_no_params_in_json() {
        assert_eq!(
            serde_json::to_string(&Problem::new("a.b")).unwrap(),
            r#"{"code":"a.b"}"#
        );
    }

    #[test]
    fn a_path_prefix_is_removed_from_every_param() {
        let problem = Problem::with("io.error", "error", "C:\\books\\x.cbz : accès refusé");
        let stripped = problem.strip_path_prefix("C:\\books\\x.cbz : ");
        assert_eq!(
            stripped.params.get("error").map(String::as_str),
            Some("accès refusé")
        );
    }
}
