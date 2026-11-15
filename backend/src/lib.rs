//! Bibliothèque `easybook` : catalogue d'un dossier de livres et lecture.
//!
//! - [`api`] : serveur HTTP (axum) + WebSocket temps réel.
//! - [`library`] : scan récursif du dossier `books/`.
//! - [`formats`] : formats reconnus — **un module par format**.
//! - [`archive`] : ouverture des archives ZIP (helpers partagés EPUB/CBZ).
//! - [`rewrite`] : réécriture des URL d'un document (HTML, CSS).
//! - [`thumbnail`] : couvertures (extraction, vignette, cache).
//! - [`pages`] : nombre de pages d'un livre (comptage, cache).
//! - [`watcher`] : interprétation des événements du système de fichiers.
//! - [`config`] : configuration (`config.yml`).

pub mod api;
pub mod archive;
pub mod config;
pub mod formats;
pub mod library;
pub mod pages;
pub mod rewrite;
pub mod thumbnail;
pub mod watcher;
