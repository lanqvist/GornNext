//! Цена экземпляра: пробивка снаряжения с фильтрами по аффиксам ложится на
//! последний выпавший экземпляр базы, а не на всю базу; переживает перезапуск.

use std::sync::Arc;

use gorn_core::engine::{Engine, EngineOptions};

fn line(t: &str, body: &str) -> String {
    format!("[2026.10.20-{t}:000][  1]TLLua: Display: [Game] {body}")
}

fn engine(data: &std::path::Path, logs: &std::path::Path) -> Engine {
    let mut e = Engine::new(
        data,
        &[],
        EngineOptions { instance_prices: true, community_prices: false, ..Default::default() },
        Arc::new(|| 1_792_500_000_000),
    )
    .unwrap();
    e.start(Some(logs), None).map_err(|(_, m)| m).unwrap();
    e
}

/// Пробивка с фильтром по аффиксу: база 4647, лоты 100…1000 ФЕ.
fn filtered_check(t: &str) -> Vec<String> {
    let mut v = vec![
        line(t, "----Socket SendMessage STT----XchgSearchPrice----SynId = 77"),
        line(t, ""),
        "+typ3 [1]".into(),
        "+filters+1+key [16]".into(),
        "|       | +refer [4647]".into(),
        "|       +2+refer [233589_d1_mt6]".into(),
        "|       | +params+1+min [2700]".into(),
        "|       | +key [5]".into(),
        line(t, "----Socket SendMessage End----"),
        line(t, "----Socket RecvMessage STT----XchgSearchPrice----SynId = 77"),
        "+prices+1+currency [100300]".into(),
    ];
    for (i, p) in [100, 200, 300, 400, 500, 600, 700, 800, 900, 1000].iter().enumerate() {
        v.push(if i == 0 { format!("|      | +unitPrices+1 [{p}]") } else { format!("|      | |          +{} [{p}]", i + 1) });
    }
    v.push(line(t, "----Socket RecvMessage End----"));
    v
}

#[test]
fn filtered_check_prices_the_last_dropped_instance() {
    let root = std::env::temp_dir().join(format!("gorn-instance-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (data, logs) = (root.join("data"), root.join("logs"));
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&logs).unwrap();

    let mut e = engine(&data, &logs);
    let mut lines = vec![
        line("10.00.00", "BagMgr@:InitBagData PageId = 100 SlotId = 0 ConfigBaseId = 100300 Num = 5"),
        // Слот 27 известен пустым — первое изменение в нём засчитывается подбором.
        line("10.00.00", "BagMgr@:RemoveBagItem PageId = 100 SlotId = 27"),
        line("10.00.01", "LevelMgr@ LevelUid, LevelType, LevelId = 1 3 5348"),
        line("10.00.01", "LevelMgr@:LevelPath, Model = Content/Art/Maps/01SD/SD_YuanGuTongDao201/SD_YuanGuTongDao201.umap World'/Game/SD'"),
        line("10.00.02", "LevelMgr:_EnterLevelSuccess() 5348"),
        line("10.00.10", "ItemChange@ ProtoName=PickItems start"),
        line("10.00.10", "BagMgr@:Modfy BagItem PageId = 100 SlotId = 27 ConfigBaseId = 4647 Num = 1"),
        line("10.00.10", "ItemChange@ ProtoName=PickItems end"),
    ];
    lines.extend(filtered_check("10.00.20"));
    e.consume(&lines);
    let s = serde_json::to_value(e.build_snapshot()).unwrap();
    let got = s["current"]["drops"].as_array().unwrap().iter().find(|d| d["configBaseId"] == 4647).cloned().unwrap();
    // P10 десяти лотов 100…1000: 100 + (200 − 100) × 0,9 = 190.
    assert_eq!(got["unitFe"].as_f64(), Some(190.0), "{got}");
    assert_eq!(got["priceSource"], "own");
    // Цена — экземпляру, не базе: в таблице цен базы 4647 нет.
    assert!(e.query_prices("").iter().all(|p| !(p.config_base_id == 4647 && p.fe == Some(190.0) && p.own_observed_at.is_some())));

    // Выход из карты — забег закрыт и записан; после перезапуска цена на месте.
    e.consume(&[line("10.01.00", "LevelMgr@ LevelUid, LevelType, LevelId = 2 0 110"), line("10.01.01", "LevelMgr:_EnterLevelSuccess() 110")]);
    e.stop();
    drop(e);
    let e = engine(&data, &logs);
    let s = serde_json::to_value(e.build_snapshot()).unwrap();
    let run = s["recent"].as_array().unwrap().iter().find(|r| r["drops"].as_array().unwrap().iter().any(|d| d["configBaseId"] == 4647)).cloned().unwrap();
    let d = run["drops"].as_array().unwrap().iter().find(|d| d["configBaseId"] == 4647).cloned().unwrap();
    assert_eq!(d["unitFe"].as_f64(), Some(190.0), "{d}");
    let _ = std::fs::remove_dir_all(&root);
}
