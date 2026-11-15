// Doublure de `$env/dynamic/public` pour les tests unitaires.
//
// `env` est vide : `apiBase()` retombe alors sur son adresse par défaut, qui est
// ce qu'on veut vérifier. Un test peut la remplir avant d'importer le module
// s'il veut éprouver le cas « API configurée ».
export const env: Record<string, string | undefined> = {};
