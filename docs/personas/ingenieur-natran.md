# Persona — Ingénieure d'études réseau (GRT gaz / Natran)

**Nom** : Camille Reynaud
**Rôle** : Ingénieure d'études réseau, opérateur GRT gaz (transport)
**Date** : juillet 2026
**Usage** : ancrer les décisions UX/UI de GazFlow sur un cas d'usage métier réel (validation de nominations — NoVa).

---

## 1. Identité

- **Âge** : 38 ans, 12 ans d'ancienneté dont 8 en étude réseau.
- **Formation** : ingénieure génie énergétique / fluides, spécialisation gaz.
- **Employeur** : un gestionnaire de réseau de transport (GRT) à l'échelle d'un pays — style Natran / GRTgaz / TEGEA. En charge d'une région réseau.
- **Localisation** : siège d'études, bureau individuel, double écran, pas sur le terrain.

## 2. Mission quotidienne

Camille **valide les nominations** des expéditeurs (shippers) contre la capacité du réseau de transport :

- Réceptionner les nominations day-ahead (quantités entry/exit par point frontière : PIR entrée, PRA/PRE sortie).
- Vérifier la **faisabilité physique** : pressions aux points de livraison ≥ bornes contractuelles, capacités des canalisations respectées.
- **Certifier ou refuser** une nomination, avec justification.
- Préparer l'**analyse N-1** : la nomination reste-t-elle faisable si un ouvrage critique tombe (compresseur, canalisation) ?
- Produire un **rapport de certification** joint à sa décision.

## 3. Objectifs et priorités

1. **Verdict rapide** : « cette nomination tient-elle ? » → réponse binaire en quelques secondes.
2. **Cause actionnable** : si non, quels points de livraison sont en déficit, de combien, et pourquoi (compresseur saturé, chemin sans source de pression, vanne fermée).
3. **Levier de négociation** : débit max faisable par sink, pour renegotier une nomination réduite avec l'expéditeur.
4. **Traçabilité** : un export prêt à joindre au dossier de certification.
5. **Débit** : beaucoup de nominations par jour → l'outil doit lui faire gagner du temps, pas lui en faire perdre.

## 4. Ce qu'elle manipule (vocabulaire métier)

- **Nomination** : ensemble entry/exit (quantités en Nm³/h ou Nm³/s) par point frontière.
- **Bornes pression contractuelles** : pression minimale à tenir aux PRA/PRE (points de livraison), pression maximale admissible.
- **PIR / PRA / PRE** : points d'interconnexion, de raccordement, de livraison.
- **Contingence N-1** : perte d'un ouvrage critique (compresseur, canalisation, source).
- **Capacité** : débit max qu'un tronçon ou un point peut transporter sous contraintes pression.
- **Linepack** : stockage gaz dans les canalisations (pour le transitoire).
- **Calage SCADA** : ajustement du modèle sur les mesures terrain.

## 5. Ce qui ne l'intéresse pas (vocabulaire solveur à éviter)

- Résidu Newton, itérations, Jacobien, paliers de continuation.
- Modes « libre / vérifier / optimiser » dans le jargon solveur.
- La liste des 582 pressions nodales en sortie.
- Les organes listés comme « equipment_states » sans lien avec son diagnostic.

## 6. Compétences techniques

- À l'aise avec : outils SIG réseau, tableurs, PIPESIM/Synergi/Myriam (selon l'opérateur), lecture de schémas P&ID, fichiers `.scn` / GasLib.
- Pas développeuse : ne lit pas le JSON de bench, n'ouvre pas un terminal.
- Lit l'anglais technique ; préfère le français pour l'UI métier.

## 7. Contexte d'usage

- **Environnement** : bureau, double écran, navigateur Chrome/Edge récent.
- **Fréquence** : quotidienne, plusieurs sessions par jour, 15-40 min par session.
- **Interruptions** : fréquentes (appels expéditeurs, opérateurs terrain) → doit pouvoir reprendre une session à l'identique.
- **Données** : réseaux de taille moyenne à grande (jusqu'à ~600 nœuds type GasLib-582).

## 8. Frustrations actuelles avec GazFlow (audit)

- Aucun verdict explicite : elle doit **déduire** la faisabilité en scrolant une colonne infinie.
- Pas d'objet « Nomination » : c'est dilué en « Demandes ».
- Vocabulaire solveur (« Demandes », « Organes », « Mode robuste (continuation) ») au lieu de vocabulaire réseau.
- Le diagnostic riche existe (trace amont, max_up vs need, capacité par sink) mais **n'est pas affiché** — il vit dans les JSON de bench.
- Aucun raccourci vers son levier réel : réduire une nomination et re-valider.
- N-1, calage et transitoire sont au même niveau que « Import » et « Exports » dans la nav — l'IA mélange écrans techniques et analyses métier.

## 9. Critère de succès (le test Camille)

> « Je charge mild_618 → je vois ⛔ Non faisable avec 4 sinks en déficit → je clique sink_88 → je vois max_up 2,64 bar vs besoin 26,0 → je lance l'étude capacité → je clique "Appliquer la capacité max partout" → je re-valide → ✅ Faisable → j'exporte le rapport de certification. »
>
> **Sans jamais ouvrir un JSON, ni scroller une liste de 582 pressions, ni lire le mot "résidu".**

## 10. Citation représentative

> « L'outil expose le moteur de calcul, pas mon métier. Je sens un front collé sur un solveur, pas un outil d'ingénieur réseau. Je veux qu'il me dise ce que je dois décider, pas ce que le solveur a calculé. »

---

## Référence

- Plan d'implémentation associé : `docs/temp/plan-interface-natran-nova.md`
- Audit UX et dialogue persona/designer : transcription de session (juillet 2026)
- Diagnostic NoVa 582 consolidé : `docs/testing/gaslib-582-compressor-diagnosis.md` (synthèse post Phase VII-bis)
