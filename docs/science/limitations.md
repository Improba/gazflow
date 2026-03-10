# Limitations du modele - GazFlow

Ce document decrit les limites connues du solveur dans son etat actuel.
Il complete `docs/science/equations.md` (modele) et `docs/science/validation.md` (tests).

## 1. Limites physiques (MVP)

- Regime permanent uniquement (pas de transitoire temporel).
- Hypothese isotherme (temperature uniforme par defaut).
- Proprietes gaz simplifiees selon le niveau de modelisation active.
- Effets gravitaires/altimetriques non pris en compte dans le flux principal.
- Compresseurs modelises en version MVP (representation simplifiee).

## 2. Limites numeriques

- La convergence depend de l'initialisation et des parametres de relaxation.
- Les reseaux tres grands peuvent necessiter des strategies de continuation et warm-start.
- Le critere de convergence reste sensible au scaling des donnees d'entree.
- Certaines configurations peuvent terminer en non-convergence explicite (mode smoke).

## 3. Limites de donnees et de validation

- La validation stricte contre une reference externe officielle peut etre incomplete selon le dataset.
- La reference interne est utile pour la non-regression, mais ne remplace pas une validation independante.
- La qualite des resultats depend directement de la qualite des fichiers de scenario et de topologie.

## 4. Impact sur l'usage

- Le solveur est adapte a des etudes techniques et comparatives de pipeline.
- Les resultats ne doivent pas etre interpretes comme une garantie d'exploitation industrielle sans calibration metier.
- Les decisions critiques (surete, contractualisation, pilotage temps reel) demandent des verifications supplementaires.

## 5. Evolutions recommandees

- Etendre la validation externe de reference (pression/debit) sur un panel de cas representatifs.
- Renforcer le modele thermo-physique (Z, viscosite, melanges) selon les besoins metier.
- Ajouter les effets gravitaires et les pertes complementaires si requis par les cas d'usage.
- Consolider les strategies de robustesse numerique pour les tres grands reseaux.

