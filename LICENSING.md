# Licences GazFlow

GazFlow est un logiciel **source disponible** (code public sur le dépôt), avec un modèle simple :

- **Gratuit** pour les particuliers et la recherche académique
- **Licence commerciale obligatoire** pour toute entreprise ou organisation

Ce n'est **pas** une licence open source au sens OSI (pas d'AGPL, pas de MIT pour l'usage entreprise).

## 1. Licence publique (gratuite)

Texte complet : [LICENSE](LICENSE) (GazFlow Public License v1.0)

### Usage gratuit autorisé

| Profil | Exemples |
|--------|----------|
| **Particulier** | Projet perso, formation, veille technique, contribution hobby |
| **Enseignement / recherche** | Université, labo public, cours, thèse (usage non commercial) |
| **Étudiant / chercheur** | Usage dans le cadre académique, y compris sur le poste de l'établissement |

Vous pouvez installer, modifier, exécuter et partager le logiciel (avec les mêmes conditions de licence).

### Usage interdit sans licence commerciale

| Profil | Exemples |
|--------|----------|
| **Entreprise** | SAS, SARL, SA, EURL, etc. |
| **Organisme public / collectivité** | GRDF, GRTgaz, régie, métropole, DREAL… |
| **Association avec activité pro** | Usage dans le cadre d'une prestation ou d'une activité économique |
| **Prestataire / ESN** | Déploiement chez un client, intégration produit |
| **Salarié ou prestataire** | Toute utilisation **pour le compte** d'une organisation ci-dessus |

**Règle simple** : si ce n'est pas un usage perso ou académique non commercial, c'est une **Entreprise** au sens de la licence → contrat obligatoire.

### Période d'évaluation

Une entreprise peut **tester** GazFlow en interne pendant **30 jours** sans licence. Au-delà : arrêt ou souscription d'une licence commerciale.

## 2. Licence commerciale (entreprises)

Obligatoire pour tout usage entreprise au-delà de l'évaluation.

Accorde notamment :

- déploiement production (interne, SaaS, appliance) ;
- modification sans obligation de publication ;
- intégration dans des prestations ou produits (selon contrat) ;
- support et mises à jour (selon offre).

Projet de contrat : [COMMERCIAL-LICENSE.md](COMMERCIAL-LICENSE.md)

**Contact** : `licensing@improba.fr`

## 3. Tableau récapitulatif

| Situation | Licence | Coût |
|-----------|---------|------|
| Ingénieur chez lui le soir, étude perso GasLib | Publique | Gratuit |
| Master / thèse à l'université | Publique | Gratuit |
| Startup qui simule son réseau en prod | **Commerciale** | Payant |
| GRDF, GRTgaz, bureau d'études, ESN | **Commerciale** | Payant |
| Collectivité / opérateur public | **Commerciale** | Payant |
| PoC interne 3 semaines chez un client | Évaluation (30 j) | Gratuit |
| PoC interne 2 mois chez un client | **Commerciale** | Payant |

> En cas de doute, écrire à `licensing@improba.fr` **avant** un déploiement.

## 4. Dépendances tierces

Cesium, Quasar, Axum, etc. restent sous leurs licences propres (Apache-2.0, MIT…). Voir `Cargo.lock` et `package-lock.json`.

## 5. Historique

- v1.0 (10 juillet 2026) : remplace MIT et le projet AGPL. Improba est l'unique ayant droit.
