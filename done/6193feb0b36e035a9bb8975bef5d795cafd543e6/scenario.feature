# language: fr
Fonctionnalité: les scénarios Gherkin d'une tâche

  Scénario: scaffolder puis lire les scénarios d'une tâche
    Étant donné une tâche "scenarios gherkin" dans l'état "todo"
    Quand je lance "tod scenario <id>"
    Alors le fichier todo/<hash>/scenario.feature est créé
    Et il commence par "# language: fr" suivi de "Fonctionnalité: scenarios gherkin..."
    Et le contenu est affiché

  Scénario: le brief de l'agent mentionne les scénarios
    Étant donné une tâche dont le dossier contient scenario.feature
    Quand je régénère son spec.md avec "tod brief <id>"
    Alors le spec.md contient une section "## Scénarios"
    Et elle demande de convertir chaque scénario en tests du framework du projet

  Scénario: la console affiche les scénarios dans le détail d'une tâche
    Étant donné le serveur tod lancé et la tâche connectée
    Quand j'ouvre le détail de la tâche dans la console
    Alors ses scénarios Gherkin apparaissent dans un bloc à part
