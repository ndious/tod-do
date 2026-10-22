# language: fr
Fonctionnalité: l'édition des specs et scénarios depuis le CLI

  Scénario: éditer la spec d'une tâche avec son éditeur
    Étant donné une tâche "edition cli" sans spec.md
    Quand je lance "tod spec <id> --edit" avec TOD_EDITOR réglé à mon éditeur
    Alors spec.md est généré puis ouvert dans l'éditeur
    Et les changements sont commités sur la branche tod

  Scénario: l'ordre de priorité de l'éditeur
    Étant donné TOD_EDITOR réglé à "code -w" et EDITOR réglé à "vim"
    Quand j'édite un fichier
    Alors c'est TOD_EDITOR qui est utilisé (avec ses flags)

  Scénario: éditer les scénarios gherkin
    Étant donné une tâche sans scenario.feature
    Quand je lance "tod scenario <id> --edit"
    Alors le scaffold français est créé puis ouvert dans l'éditeur
    Et sans --edit le contenu est simplement affiché
