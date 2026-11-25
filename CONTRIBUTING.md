# Contributing to a public repository

**EN** · This repository is public (MIT). A push is a publication: branches, draft pull requests, comments and the author
of each commit are visible to anyone, and a commit may stay reachable by its hash after a force push or a deletion.

**FR** · Ce dépôt est public (MIT). Pousser, c'est publier : branches, PR en brouillon, commentaires et auteur des commits
sont visibles par tous, et un commit peut rester accessible par son numéro même après suppression.

## Never here / Jamais ici

- a key, a token, a password, a `.env` file, the address of a machine of the team, a path of a personal computer;
- code of the private repositories (`module-kits` and the others) beyond what `tools/sync-sdk` copies on purpose;
- a customer's game, an asset whose licence does not allow it, an unannounced product plan;
- a personal e-mail address: use GitHub's `noreply` address.

## Before pushing / Avant de pousser

1. `git remote -v`: is it really this repository you want? / est-ce bien ce dépôt ?
2. `git log --format='%an <%ae>' origin/main..HEAD`: only `noreply` addresses / seulement des adresses `noreply`.
3. `git diff origin/main...HEAD`: read it whole, as a stranger would / le relire en entier, comme un inconnu.

## `tools/sync-sdk`

It copies **whole folders** of the private repository (`core/abi/src`, `core/kit-guest/src`, `core/kit-access/src`,
`core/contracts/src`): a file added there becomes public at the next copy. Read the diff of `sdk/` before committing it.

## Something private went out? / Quelque chose de privé est sorti ?

Revoke the key first, then tell the team: deleting the branch is not enough. / Révoquer la clé d'abord, prévenir ensuite :
supprimer la branche ne suffit pas.
