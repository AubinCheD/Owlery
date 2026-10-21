//! Entrées fictives (`OWLERY_DEMO=1`) : pour essayer l'interface et la capturer sans compte réel.

use crate::model::{Ci, Forge, Item, Kind};

pub fn items(now: i64) -> Vec<Item> {
    let e = |repo: &str, n: u64, kind: Kind, title: &str, ago_min: i64, ci: Option<Ci>, branch: Option<&str>| Item {
        id: Item::make_id(Forge::GitHub, repo, Some(n), kind),
        forge: Forge::GitHub,
        kind,
        repo: repo.into(),
        number: Some(n),
        title: title.into(),
        url: format!("https://github.com/{repo}/pull/{n}"),
        branch: branch.map(str::to_owned),
        author: "camille".into(),
        updated_at: now - ago_min * 60,
        ci,
        summary: "Exemple fictif : la description de la PR ou du ticket apparaît ici, sur une ligne.".into(),
    };
    vec![
        e("contoso/api", 142, Kind::ReviewRequested, "Ajoute la pagination", 12, Some(Ci::Success), Some("pagination")),
        e("contoso/api", 138, Kind::CiFailed, "Corrige l'export CSV", 40, Some(Ci::Failure), Some("export-csv")),
        e("contoso/api", 131, Kind::MyPr, "Met à jour les dépendances", 300, Some(Ci::Pending), Some("deps")),
        e("fabrikam/web", 57, Kind::Mention, "Question sur le cache des images", 90, None, None),
        e(
            "fabrikam/web",
            61,
            Kind::ReviewRequested,
            "Nouvelle page d'accueil",
            1500,
            Some(Ci::Success),
            Some("accueil"),
        ),
        e("northwind/app", 12, Kind::Assigned, "Erreur à l'export", 2900, None, None),
    ]
}
