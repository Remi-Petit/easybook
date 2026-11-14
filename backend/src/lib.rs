//! Bibliothèque `easybook` : catalogue d'un dossier de livres et lecture.
//!
//! - [`api`] : serveur HTTP (axum) + WebSocket temps réel.
//! - [`library`] : scan récursif du dossier `books/`.
//! - [`formats`] : formats reconnus (EPUB, CBZ, CBR, PDF).
//! - [`archive`] : ouverture des archives ZIP (EPUB et CBZ).
//! - [`epub`] : sommaire et chapitres d'un EPUB.
//! - [`rewrite`] : réécriture des URL d'un document (HTML, CSS).
//! - [`thumbnail`] : couvertures (extraction, vignette, cache).
//! - [`watcher`] : interprétation des événements du système de fichiers.
//! - [`config`] : configuration (`config.yml`).

pub mod api;
pub mod archive;
pub mod config;
pub mod epub;
pub mod formats;
pub mod library;
pub mod rewrite;
pub mod thumbnail;
pub mod watcher;
