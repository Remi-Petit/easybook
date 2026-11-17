import http from 'k6/http';
import { fail } from 'k6';
import { BASE_URL } from './config.js';

const MAX_PER_LIST = 300;

// Échantillon régulier : garde la diversité d'une grosse bibliothèque sans
// copier des milliers de livres dans chaque VU.
function sample(list) {
  const stride = Math.max(1, Math.ceil(list.length / MAX_PER_LIST));
  return list.filter((_, i) => i % stride === 0).map((b) => ({
    rel: b.rel,
    folder: b.folder,
    format: b.format,
    pages: b.pages || 1,
    modified: b.modified || 0,
    size: b.size,
  }));
}

// Lit le catalogue réel : les scénarios tapent sur de vrais livres.
export function discover() {
  const res = http.get(`${BASE_URL}/api/books`, { tags: { name: 'setup_books' } });
  if (res.status !== 200) {
    fail(`GET ${BASE_URL}/api/books → ${res.status} : l'application est-elle joignable ?`);
  }
  const scan = res.json();
  const openable = scan.books.filter((b) => b.readable && b.pages);

  return {
    total: scan.total,
    books: sample(scan.books),
    folders: scan.folders.slice(0, 100).map((f) => f.rel),
    comic: sample(openable.filter((b) => b.format === 'cbz')),
    epub: sample(openable.filter((b) => b.format === 'epub')),
    pdf: sample(scan.books.filter((b) => b.format === 'pdf')),
  };
}
