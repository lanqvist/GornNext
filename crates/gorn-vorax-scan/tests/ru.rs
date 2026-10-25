//! Не в эталоне: русские заголовки карт. Тексты — то, что Windows OCR (ru) прочитал на вырезках заголовков
//! (`title_crop`) с кадров живых забегов 25.10.2026 на 3440×1440, как есть: с обрезанными краями, строками
//! описания и обрывками соседних карт. Каждый должен уверенно (с зазором, как в read_titles) дать свою карту.

use gorn_vorax_scan::scan_data_ru;
use gorn_vorax_scan::titles::ranked;

const READ: &[(&str, &str, &str)] = &[
    ("instrument", "Выводковая камера\nПри наличии хотя бы 2", "Incubation Sac"),
    ("potion", "Обезболивающа\nнастойка - Агит\nВыбирает 1 груп\nмонстров для м at\nмул\nвы", "Anesthetic Tincture - Agito"),
    ("potion", "Мутагенный поро\nВыбирает 1-2 груп\nмонстров в\nСил\nпорс\nм", "Mutagenic Powder"),
    ("potion", "Сильнодеиствующий\nпорошок для удаления\nВыбирает 1 групп\nмонстров в +154", "Potent Xenopurge Powder"),
    ("potion", "Одури алплбющая\nнастойка\nВыбирает 1 групп\nионстров, Пробуждая\nвы\nмон", "Bewitching Tincture"),
    ("potion", "Раствор для линьки\nВыбирает 1-2 группы\nмонстров для зи", "Slough Solution"),
    ("potion", "Порошок для тонких\nконечностей - Паразит\nДобавить 1 групп\nзитов,увеличив их", "Slender-Limb Powder - Pest"),
    ("potion", "Порошок для\nудаления\nВыбирает 1 груп\nмонстров в +127\nРаст,\nмозгог\nВыби\nмон(", "Xenopurge Powder"),
    ("potion", "Расть.\nюзговой оболочки\nВыбирает 1 групп\nмонстров с +30\nгор\nмонс", "Leptomeningeal Solution"),
    ("potion", "Гормон олјвнецов -\nПаразит\nВыбирает 1 групп\nмонстров, мутирует их", "Twin Hormone - Pest"),
    ("potion", "Гормон Оллзнецо\nСкелетный стра\nВыбирает 1 груп\nлонстров, мутируя\nПоро[\nспин\nСлучайн\nболее", "Twin Hormone - Bone Sentry"),
    ("potion", "Порош— свежего\nспинного мозга\nручайно выберите не\nболее З групп не-\nАптч", "Fresh Spinal Powder"),
    ("potion", "Аптечка (маленькая)\nСодержит З групп\nслучайных зелий", "Potion Kit (Small)"),
    ("potion", "Раствор серого вещества\nиз спинного мозга\nСлучайно выбирает 1\nгрупп монстров", "Gray Matter Spinal Solution"),
    ("potion", "цор\nуп\n_у.д\nРаствор СПИННОГО\nмозга - Агито\nДобавляет 1 групп\nНАгито, увеличив их", "Spinal Solution - Agito"),
    ("instrument", "Напильник с зубья•\nВ конце хода все\nиз\nмаг", "Canine Teeth File"),
    ("instrument", "ремень\nиз сыромятной кожи\nПри наличии З групп\nмагических монстров,\nкаждый ход с", "Rawhide Restraint"),
    ("instrument", "1йца насекомых\nналичии хотя бы З", "Clustered Eggs"),
    ("potion", "Инородный гормон\nВыбирает 1--2 групп\nмонстров для", "Xeno-Hormone"),
    ("potion", "си\nРаствор густой же\nпор\nВыбирает 1 груп\nмонстров и М и", "Viscous Bile Solution"),
    ("potion", "Чисть:л раствор\nживых пиявок\nВыбирает 1 групп\nмонстров, чтобы\nмонстры той же 2", "Pure Leech Solution"),
    ("potion", "их\nзит\nкол\nПорошок для\nразмножения\nВыбирает случайн\nгрупп монстров", "Vivigenic Powder"),
    ("potion", "Обезболивающа\nпо\nнастойка - Чужон коне(\nВыбирает 1 груп\nмонстров, мутируе", "Anesthetic Tincture - Aemberon"),
    ("potion", "Порошок для тонких\nконечностей - Скелетны...\nДобавить 1 групп\nСкелетных", "Slender-Limb Powder - Bone Sentry"),
    ("potion", "Смешанныл раств\nживых пиявок\nВыбирает 1 груп\nмонстров, чтобы\nм", "Compound Leech Solution"),
    ("potion", "Раствор\nлщеварительных ферм...\nВыбирает 1 групп\nмонстров с +61\nпс\nкон\n(1T0[", "Digestive Enzyme Solution"),
    ("potion", "Раствор из полого\nспинного мозга\nВыберите группу\nмонстров 1 с", "Hollow Spinal Solution"),
    // второй забег 25.10
    ("potion", "для\nразмягчения кост\nЕсли у вас есть хотя\n1 групп е; Скелетн\nпс\nкон\n(1T0[", "Osteolytic Ointment"),
    ("instrument", "Извиь=дийся\nспинной мозг\nКаждый раз при\nдобавлении монст\nпри", "Vermicular Spinal Cord"),
    ("instrument", "Чугунная пила\nВ конце хода монстры,\nколичество и", "Pig Iron Bone Saw"),
    ("instrument", "(э\nВеревка ; — -јАOЖИ\nживотных\nВ конце хода, есл\nвас есть хотя бы\nпри", "Sinew Garrote"),
    ("instrument", "Шестиса,—\nлетровый\nчерепной штифт\nПри добавлении\nмонстров, если их", "Two-Inch Skull Nail"),
    ("potion", "Порошах из яичн\nскорлупы\nВыбирает случайн\nгрупп монстров+\nсме", "Eggshell Powder"),
    ("potion", "Раствор зеленой\nжелчи\nВыбирает 1-2 гру\nмонстров для м а", "Green Bile Solution"),
    ("potion", "при,\n-4 для\nнасекомых\nДобавьте 4 групп\nы- паразитов; за", "Insect Attractant"),
    ("protocol", "Лазерная а:лпутац\nВо время финальн\nбитвы появляютс\nиглс\nво\nбил", "Laser Amputation"),
    ("protocol", "Асептическая зацџ\nДополнительно -2\nк урону, получаем\nКог,\nпо", "Aseptic Isolation Protocol"),
    ("protocol", "Протезировани\nлобной доли\nКогда редкие монс\nполучают урон, о\nУда\nбр\nмон", "Frontal Lobe Repair"),
    ("protocol", "Удал из\nбрюшной полости\nПосле поражения\nмонстры оставляют", "Laparolithotomy"),
    ("protocol", "Стиглуляция\nспинномозговых нер\nПри появлении\nмонстра, его скоро\nво", "Spinal Cord Stimulation"),
    ("protocol", "Трепанацлл черепа\nВо время финальной\nбитвы случайным", "Trepanation"),
    // третий забег 25.10
    ("instrument", "Сросшиеся кост\nПри наличии хотя б\nгрупп Скелетн", "Adherent Metatarsals"),
    ("instrument", "зл\nСжатые когти\nПри убирании\nмонстров не\nмут", "Contracted Claws"),
    ("instrument", "— ' 37 •\nЗлокач—, денный\nнарост\nКогда монстрь[\nмутируют в ЧУЖИХ", "Aberrant Sprout"),
    ("instrument", "Грязныл распато\nВ конце хода все\nколичеством вы\nРа:", "Fouled Scraper"),
    ("instrument", "Развитая кора мо\nВ конце хода\nслучайные 2 груп\nВерк\nва", "Sloughed Cortex"),
    ("potion", "Мазь от срурункул\nВыбирает 1 груп\nмонстров с +20\n06\nмонс", "Cleansing Ointment"),
    ("potion", "Порошок для рос\nкостей\nВыбирает случайн\nгрупп монстров и д", "Osteogenic Powder"),
    ("potion", "Гормон активаци\nразвития яиц\nДобавляет 1 груп\nпаразитов С\nторс\nэпи,\nЕсли\nго", "Viviparous Hormone"),
    ("potion", "Торфянс\n,лресс из\nэпидемической зоны\nЕсли у вас есть хотя бы\n1 групп Скелетных", "Plague Quarter Peat Poultice"),
    ("protocol", "по\nвведению миорелакс\nИгроки продолжа\nподвергаться рис\nво\nб", "Muscle Relaxant Injection Experiment"),
];

#[test]
fn russian_titles_pick_their_card() {
    let data = scan_data_ru();
    let mut wrong = Vec::new();
    for (phase, text, want) in READ {
        let r = ranked(data, phase, text);
        let gap = r[0].1 - r.get(1).map_or(f64::NEG_INFINITY, |x| x.1);
        if r[0].0 != *want || r[0].1 < 6.0 || gap < 3.0 {
            wrong.push(format!("{want}: {:?} (зазор {gap:.1}) по {text:?}", &r[..2.min(r.len())]));
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

#[test]
fn english_text_still_matches_english_names() {
    let r = ranked(scan_data_ru(), "potion", "Mutagenic Powder\nSelect 1-2 groups");
    assert_eq!(r[0].0, "Mutagenic Powder");
}
