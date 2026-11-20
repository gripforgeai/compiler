# gripforge-kit-guest

Le côté module des kits : une façade de l'hôte (`Host`, `Kx`) au-dessus des appels vers le cœur, pour qu'une source de kit se compile sans changement en bibliothèque native (`kithost.native@2`) ou en wasm. Reprise de MKP `kits/kit_guest` ([sources](../../docs/sources.md)).

- Features : `common-memory` (wasm relogeable dans la mémoire commune, allocateur du cœur), `isolated` (kit tiers avec sa propre mémoire), `entry-probe` (mesures seulement).
- En natif, `common_memory!` ne pose pas l'allocateur du cœur : chaque bibliothèque garde l'allocateur système (décision D3).
- `socle.json` : la base de construction d'un module de kit.

## Construire et tester

```sh
cargo test -p gripforge-kit-guest
```

## Voir aussi

- [Deux façades pour une crate partagée](../../docs/deux-facades.md)
- [Modules de kit](../../docs/modules-de-kit.md)
