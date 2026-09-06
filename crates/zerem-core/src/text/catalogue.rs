//! The strings themselves. Nothing here does anything.
//!
//! Split from the code that reads it because they are two jobs with two
//! audiences: [`super`] is read by whoever changes how translation works, and
//! this file by whoever adds a language. Neither should have to scroll through
//! the other.
//!
//! **The order of every array is [`crate::language::SHIPPED`]'s order**, and a
//! test in [`super`] says so. For [`TABLE`] that means English is missing — the
//! key is the English — so its rows are one shorter than the rest.
//!
//! Plural arrays hold *as many forms as the sentence needs in that language*,
//! not as many as the language has. Indonesian, Vietnamese and Turkish do not
//! mark a noun after a numeral, so one form is the whole truth; Russian needs
//! three. Arabic has six, and the entries here are written to not agree with
//! the number at all — which is what Arabic interface translators do, because
//! six forms of a sentence nobody reads twice is a cost with no reader.

/// Fixed strings, keyed by the English original.
///
/// Sorted by key: the lookup is a binary search and a test enforces it.
pub(super) const TABLE: [(&str, [&str; 10]); 35] = [
    (
        "Another program has one of the files open",
        [
            "برنامج آخر يفتح أحد الملفات",
            "Ein anderes Programm hat eine der Dateien geöffnet",
            "Otro programa tiene uno de los archivos abierto",
            "Un autre programme a ouvert l'un des fichiers",
            "कोई दूसरा प्रोग्राम इनमें से एक फ़ाइल खोले हुए है",
            "Program lain sedang membuka salah satu berkas",
            "Outro programa está com um dos arquivos aberto",
            "Один из файлов открыт в другой программе",
            "Dosyalardan biri başka bir programda açık",
            "Một chương trình khác đang mở một trong các tệp",
        ],
    ),
    (
        "Checking",
        [
            "جارٍ التحقق",
            "Wird geprüft",
            "Verificando",
            "Vérification",
            "जाँच हो रही है",
            "Memeriksa",
            "Verificando",
            "Проверка",
            "Denetleniyor",
            "Đang kiểm tra",
        ],
    ),
    (
        "Connecting",
        [
            "جارٍ الاتصال",
            "Verbinden",
            "Conectando",
            "Connexion",
            "कनेक्ट हो रहा है",
            "Menghubungkan",
            "Conectando",
            "Подключение",
            "Bağlanıyor",
            "Đang kết nối",
        ],
    ),
    // The table's own column headings, which reached the window as plain Rust
    // strings and were English in all eleven languages. Nothing caught it: the
    // `.po` test compares the `.slint` against the catalogues, and these are in
    // neither — `sort::TITLES` went straight to `set_col_title` without passing
    // through `tr` at all.
    //
    // Short on purpose. These sit over columns a person drags narrow, and a
    // heading that elides is a heading that says less than the number under it.
    (
        "Down",
        [
            "تنزيل",
            "Runter",
            "Bajada",
            "Réception",
            "डाउन",
            "Unduh",
            "Descida",
            "Приём",
            "İndirme",
            "Tải xuống",
        ],
    ),
    (
        "Downloading",
        [
            "جارٍ التنزيل",
            "Wird heruntergeladen",
            "Descargando",
            "Téléchargement",
            "डाउनलोड हो रहा है",
            "Mengunduh",
            "Baixando",
            "Загрузка",
            "İndiriliyor",
            "Đang tải xuống",
        ],
    ),
    (
        "ETA",
        [
            "الوقت المتبقي",
            "Restzeit",
            "Restante",
            "Restant",
            "शेष समय",
            "Sisa waktu",
            "Restante",
            "Осталось",
            "Kalan",
            "Còn lại",
        ],
    ),
    ("Error", ["خطأ", "Fehler", "Error", "Erreur", "त्रुटि", "Galat", "Erro", "Ошибка", "Hata", "Lỗi"]),
    (
        "Fetching metadata",
        [
            "جلب البيانات الوصفية",
            "Metadaten werden geholt",
            "Obteniendo metadatos",
            "Récupération des métadonnées",
            "मेटाडेटा लिया जा रहा है",
            "Mengambil metadata",
            "Buscando metadata",
            "Получение метаданных",
            "Üstveri alınıyor",
            "Đang lấy siêu dữ liệu",
        ],
    ),
    (
        "Magnet link copied",
        [
            "تم نسخ رابط المغناطيس",
            "Magnet-Link kopiert",
            "Enlace magnet copiado",
            "Lien magnet copié",
            "मैग्नेट लिंक कॉपी हुआ",
            "Tautan magnet disalin",
            "Link magnet copiado",
            "Magnet-ссылка скопирована",
            "Magnet bağlantısı kopyalandı",
            "Đã sao chép liên kết magnet",
        ],
    ),
    ("Name", ["الاسم", "Name", "Nombre", "Nom", "नाम", "Nama", "Nome", "Имя", "Ad", "Tên"]),
    (
        "No files selected",
        [
            "لم يتم اختيار أي ملف",
            "Keine Dateien ausgewählt",
            "Ningún archivo seleccionado",
            "Aucun fichier sélectionné",
            "कोई फ़ाइल चुनी नहीं गई",
            "Tidak ada berkas dipilih",
            "Nenhum arquivo selecionado",
            "Файлы не выбраны",
            "Hiçbir dosya seçilmedi",
            "Chưa chọn tệp nào",
        ],
    ),
    (
        "No limit",
        [
            "بلا حد",
            "Ohne Limit",
            "Sin límite",
            "Sans limite",
            "कोई सीमा नहीं",
            "Tanpa batas",
            "Sem limite",
            "Без ограничения",
            "Sınırsız",
            "Không giới hạn",
        ],
    ),
    (
        "No one is sharing",
        [
            "لا أحد يشارك",
            "Niemand teilt",
            "Nadie está compartiendo",
            "Personne ne partage",
            "कोई साझा नहीं कर रहा",
            "Tidak ada yang membagikan",
            "Ninguém está compartilhando",
            "Никто не раздаёт",
            "Paylaşan yok",
            "Không có ai chia sẻ",
        ],
    ),
    (
        "No peers found",
        [
            "لم يتم العثور على أقران",
            "Keine Peers gefunden",
            "No se encontraron pares",
            "Aucun pair trouvé",
            "कोई पीयर नहीं मिला",
            "Tidak ada peer ditemukan",
            "Nenhum peer encontrado",
            "Пиры не найдены",
            "Eş bulunamadı",
            "Không tìm thấy peer",
        ],
    ),
    (
        "No permission to write in the download folder",
        [
            "لا إذن بالكتابة في مجلد التنزيل",
            "Keine Schreibrechte im Download-Ordner",
            "Sin permiso para escribir en la carpeta de descargas",
            "Pas d'autorisation d'écriture dans le dossier de téléchargement",
            "डाउनलोड फ़ोल्डर में लिखने की अनुमति नहीं",
            "Tidak ada izin menulis di folder unduhan",
            "Sem permissão para escrever na pasta de destino",
            "Нет прав на запись в папку загрузок",
            "İndirme klasörüne yazma izni yok",
            "Không có quyền ghi vào thư mục tải về",
        ],
    ),
    (
        "Nobody answered with the file list",
        [
            "لم يرد أحد بقائمة الملفات",
            "Niemand hat mit der Dateiliste geantwortet",
            "Nadie respondió con la lista de archivos",
            "Personne n'a répondu avec la liste des fichiers",
            "किसी ने फ़ाइल सूची नहीं भेजी",
            "Tidak ada yang menjawab dengan daftar berkas",
            "Ninguém respondeu com a lista de arquivos",
            "Никто не прислал список файлов",
            "Dosya listesiyle kimse yanıt vermedi",
            "Không ai trả lời kèm danh sách tệp",
        ],
    ),
    (
        "Not enough space on the disk",
        [
            "لا توجد مساحة كافية على القرص",
            "Nicht genug Speicherplatz auf dem Datenträger",
            "No hay espacio suficiente en el disco",
            "Espace insuffisant sur le disque",
            "डिस्क में पर्याप्त जगह नहीं",
            "Ruang disk tidak cukup",
            "Sem espaço no disco",
            "Недостаточно места на диске",
            "Diskte yeterli alan yok",
            "Không đủ dung lượng đĩa",
        ],
    ),
    (
        "Paused",
        [
            "متوقف مؤقتًا",
            "Angehalten",
            "En pausa",
            "En pause",
            "रुका हुआ",
            "Dijeda",
            "Pausado",
            "Приостановлено",
            "Duraklatıldı",
            "Đã tạm dừng",
        ],
    ),
    ("Peers", ["الأقران", "Peers", "Pares", "Pairs", "पीयर", "Peer", "Peers", "Пиры", "Eşler", "Peer"]),
    (
        "Queued",
        [
            "في الانتظار",
            "In Warteschlange",
            "En cola",
            "En file d'attente",
            "कतार में",
            "Dalam antrean",
            "Na fila",
            "В очереди",
            "Sırada",
            "Trong hàng đợi",
        ],
    ),
    (
        "Ratio",
        [
            "النسبة",
            "Verhältnis",
            "Proporción",
            "Ratio",
            "अनुपात",
            "Rasio",
            "Proporção",
            "Рейтинг",
            "Oran",
            "Tỷ lệ",
        ],
    ),
    (
        "Seeding",
        [
            "جارٍ البذر",
            "Wird verteilt",
            "Compartiendo",
            "Partage",
            "सीड हो रहा है",
            "Menyebarkan",
            "Semeando",
            "Раздача",
            "Gönderiliyor",
            "Đang chia sẻ",
        ],
    ),
    (
        "Size",
        ["الحجم", "Größe", "Tamaño", "Taille", "आकार", "Ukuran", "Tamanho", "Размер", "Boyut", "Kích thước"],
    ),
    (
        "State",
        [
            "الحالة",
            "Status",
            "Estado",
            "État",
            "स्थिति",
            "Status",
            "Estado",
            "Состояние",
            "Durum",
            "Trạng thái",
        ],
    ),
    (
        "Streaming is not available",
        [
            "البث غير متاح",
            "Streaming ist nicht verfügbar",
            "La reproducción en directo no está disponible",
            "La lecture en continu n'est pas disponible",
            "स्ट्रीमिंग उपलब्ध नहीं है",
            "Streaming tidak tersedia",
            "Reprodução ao vivo indisponível",
            "Потоковое воспроизведение недоступно",
            "Akış kullanılamıyor",
            "Không thể phát trực tuyến",
        ],
    ),
    (
        "System",
        [
            "النظام",
            "System",
            "Sistema",
            "Système",
            "सिस्टम",
            "Sistem",
            "Sistema",
            "Системный",
            "Sistem",
            "Hệ thống",
        ],
    ),
    (
        "That drive is not available",
        [
            "محرك الأقراص هذا غير متاح",
            "Dieses Laufwerk ist nicht verfügbar",
            "Esa unidad no está disponible",
            "Ce lecteur n'est pas disponible",
            "वह ड्राइव उपलब्ध नहीं है",
            "Drive itu tidak tersedia",
            "Essa unidade não está disponível",
            "Этот диск недоступен",
            "O sürücü kullanılamıyor",
            "Ổ đĩa đó không khả dụng",
        ],
    ),
    (
        "That file has not finished yet",
        [
            "لم يكتمل هذا الملف بعد",
            "Diese Datei ist noch nicht fertig",
            "Ese archivo aún no ha terminado",
            "Ce fichier n'est pas encore terminé",
            "वह फ़ाइल अभी पूरी नहीं हुई",
            "Berkas itu belum selesai",
            "Esse arquivo ainda não terminou",
            "Этот файл ещё не завершён",
            "O dosya henüz tamamlanmadı",
            "Tệp đó chưa tải xong",
        ],
    ),
    (
        "That file is not on disk yet",
        [
            "هذا الملف ليس على القرص بعد",
            "Diese Datei liegt noch nicht auf dem Datenträger",
            "Ese archivo aún no está en el disco",
            "Ce fichier n'est pas encore sur le disque",
            "वह फ़ाइल अभी डिस्क पर नहीं है",
            "Berkas itu belum ada di disk",
            "Esse arquivo ainda não está no disco",
            "Этого файла ещё нет на диске",
            "O dosya henüz diskte değil",
            "Tệp đó chưa có trên đĩa",
        ],
    ),
    (
        "That torrent has no folder yet",
        [
            "لا يوجد مجلد لهذا التورنت بعد",
            "Dieser Torrent hat noch keinen Ordner",
            "Ese torrent aún no tiene carpeta",
            "Ce torrent n'a pas encore de dossier",
            "उस टोरेंट का अभी कोई फ़ोल्डर नहीं है",
            "Torrent itu belum punya folder",
            "Esse torrent ainda não tem pasta",
            "У этого торрента ещё нет папки",
            "O torrentin henüz klasörü yok",
            "Torrent đó chưa có thư mục",
        ],
    ),
    (
        "That torrent has no infohash yet",
        [
            "لا يوجد infohash لهذا التورنت بعد",
            "Dieser Torrent hat noch keinen Infohash",
            "Ese torrent aún no tiene infohash",
            "Ce torrent n'a pas encore d'infohash",
            "उस टोरेंट का अभी कोई infohash नहीं है",
            "Torrent itu belum punya infohash",
            "Esse torrent ainda não tem infohash",
            "У этого торрента ещё нет infohash",
            "O torrentin henüz infohash'i yok",
            "Torrent đó chưa có infohash",
        ],
    ),
    (
        "The clipboard has no magnet link in it",
        [
            "لا يوجد رابط مغناطيس في الحافظة",
            "In der Zwischenablage ist kein Magnet-Link",
            "No hay ningún enlace magnet en el portapapeles",
            "Le presse-papiers ne contient aucun lien magnet",
            "क्लिपबोर्ड में कोई मैग्नेट लिंक नहीं है",
            "Tidak ada tautan magnet di papan klip",
            "Não há link magnet na área de transferência",
            "В буфере обмена нет magnet-ссылки",
            "Panoda magnet bağlantısı yok",
            "Bảng nhớ tạm không có liên kết magnet",
        ],
    ),
    (
        "The download folder is not there any more",
        [
            "لم يعد مجلد التنزيل موجودًا",
            "Der Download-Ordner existiert nicht mehr",
            "La carpeta de descargas ya no existe",
            "Le dossier de téléchargement n'existe plus",
            "डाउनलोड फ़ोल्डर अब मौजूद नहीं है",
            "Folder unduhan sudah tidak ada",
            "A pasta de destino não existe mais",
            "Папки загрузок больше нет",
            "İndirme klasörü artık yok",
            "Thư mục tải về không còn nữa",
        ],
    ),
    (
        "The download folder is read-only",
        [
            "مجلد التنزيل للقراءة فقط",
            "Der Download-Ordner ist schreibgeschützt",
            "La carpeta de descargas es de solo lectura",
            "Le dossier de téléchargement est en lecture seule",
            "डाउनलोड फ़ोल्डर केवल पढ़ने योग्य है",
            "Folder unduhan hanya-baca",
            "A pasta de destino é somente leitura",
            "Папка загрузок доступна только для чтения",
            "İndirme klasörü salt okunur",
            "Thư mục tải về chỉ đọc",
        ],
    ),
    ("Up", ["رفع", "Hoch", "Subida", "Envoi", "अप", "Unggah", "Subida", "Отдача", "Gönderme", "Tải lên"]),
];

/// "12 of 300" — how much of the list a filter is showing.
pub(super) const MATCHED: [&str; 11] = [
    "{0} of {1}",
    "{0} من {1}",
    "{0} von {1}",
    "{0} de {1}",
    "{0} sur {1}",
    "{1} में से {0}",
    "{0} dari {1}",
    "{0} de {1}",
    "{0} из {1}",
    "{1} içinde {0}",
    "{0} trên {1}",
];

/// What the add dialog says when the download will not fit.
///
/// Not one template with the words shuffled around it. Portuguese puts a verb
/// where English puts a noun, Russian counts what is still needed rather than
/// what is missing, and a translator who only moved the placeholder would have
/// produced something nobody says out loud.
pub(super) const SHORTFALL: [&str; 11] = [
    "Not enough room in this folder — {0} short",
    "لا توجد مساحة كافية في هذا المجلد — ينقص {0}",
    "Nicht genug Platz in diesem Ordner — {0} fehlen",
    "No cabe en esta carpeta — faltan {0}",
    "Pas assez de place dans ce dossier — il manque {0}",
    "इस फ़ोल्डर में जगह नहीं है — {0} कम है",
    "Ruang di folder ini tidak cukup — kurang {0}",
    "Não cabe nesta pasta — faltam {0}",
    "В этой папке не хватает места — нужно ещё {0}",
    "Bu klasörde yeterli yer yok — {0} eksik",
    "Thư mục này không đủ chỗ — thiếu {0}",
];

/// A file in the watched folder that could not be added, and which one.
pub(super) const WATCH_FAILED: [&str; 11] = [
    "Could not add {0} from the watched folder",
    "تعذر إضافة {0} من المجلد المراقَب",
    "{0} konnte nicht aus dem überwachten Ordner hinzugefügt werden",
    "No se pudo añadir {0} desde la carpeta vigilada",
    "Impossible d'ajouter {0} depuis le dossier surveillé",
    "निगरानी वाले फ़ोल्डर से {0} जोड़ा नहीं जा सका",
    "Tidak dapat menambahkan {0} dari folder yang dipantau",
    "Não foi possível adicionar {0} da pasta vigiada",
    "Не удалось добавить {0} из отслеживаемой папки",
    "İzlenen klasörden {0} eklenemedi",
    "Không thể thêm {0} từ thư mục đang theo dõi",
];

/// The clipboard would not answer, and what it said about it.
pub(super) const CLIPBOARD_FAILED: [&str; 11] = [
    "Could not reach the clipboard: {0}",
    "تعذر الوصول إلى الحافظة: {0}",
    "Zwischenablage nicht erreichbar: {0}",
    "No se pudo acceder al portapapeles: {0}",
    "Impossible d'accéder au presse-papiers : {0}",
    "क्लिपबोर्ड तक नहीं पहुँच सके: {0}",
    "Tidak dapat mengakses papan klip: {0}",
    "Não foi possível acessar a área de transferência: {0}",
    "Не удалось обратиться к буферу обмена: {0}",
    "Panoya erişilemedi: {0}",
    "Không thể truy cập bảng nhớ tạm: {0}",
];

/// A finished download that could not be moved to where finished ones are kept.
pub(super) const MOVE_FAILED: [&str; 11] = [
    "Could not move the finished download: {0}",
    "تعذر نقل التنزيل المكتمل: {0}",
    "Der fertige Download konnte nicht verschoben werden: {0}",
    "No se pudo mover la descarga terminada: {0}",
    "Impossible de déplacer le téléchargement terminé : {0}",
    "पूरा हुआ डाउनलोड नहीं ले जाया जा सका: {0}",
    "Tidak dapat memindahkan unduhan yang selesai: {0}",
    "Não foi possível mover o download terminado: {0}",
    "Не удалось переместить завершённую загрузку: {0}",
    "Tamamlanan indirme taşınamadı: {0}",
    "Không thể di chuyển tệp tải xong: {0}",
];

/// What the status bar says when a download lands.
pub(super) const FINISHED_ONE: [&str; 11] = [
    "{0} finished",
    "اكتمل {0}",
    "{0} ist fertig",
    "{0} ha terminado",
    "{0} est terminé",
    "{0} पूरा हुआ",
    "{0} selesai",
    "{0} terminou",
    "{0} завершён",
    "{0} tamamlandı",
    "{0} đã xong",
];

/// And when several land in the same second.
///
/// Only ever called with more than one, so English needs no singular — the
/// arrays here are as long as the sentence needs, and [`super::pick`] holds at
/// the last form for anything past it.
pub(super) const FINISHED_MANY: [&[&str]; 11] = [
    &["{0} downloads finished"],
    &["اكتمل {0} من التنزيلات"],
    &["{0} Downloads abgeschlossen"],
    &["{0} descargas terminadas"],
    &["{0} téléchargements terminés"],
    &["{0} डाउनलोड पूरे हुए"],
    &["{0} unduhan selesai"],
    &["{0} downloads terminaram"],
    &["{0} загрузка завершена", "{0} загрузки завершены", "{0} загрузок завершено"],
    &["{0} indirme tamamlandı"],
    &["{0} tệp tải xong"],
];

/// "3 files first", with nothing else waiting behind them.
pub(super) const FETCHING_FIRST: [&[&str]; 11] = [
    &["{0} file first", "{0} files first"],
    &["{0} من الملفات أولًا"],
    &["{0} Datei zuerst", "{0} Dateien zuerst"],
    &["{0} archivo primero", "{0} archivos primero"],
    &["{0} fichier d'abord", "{0} fichiers d'abord"],
    &["{0} फ़ाइल पहले", "{0} फ़ाइलें पहले"],
    &["{0} berkas didahulukan"],
    &["{0} arquivo na frente", "{0} arquivos na frente"],
    &["{0} файл первым", "{0} файла первыми", "{0} файлов первыми"],
    &["{0} dosya önce"],
    &["{0} tệp trước"],
];

/// "1 file first · 11 waiting" — the same, with the queue behind it.
pub(super) const FETCHING_FIRST_WAITING: [&[&str]; 11] = [
    &["{0} file first · {1} waiting", "{0} files first · {1} waiting"],
    &["{0} من الملفات أولًا · {1} في الانتظار"],
    &["{0} Datei zuerst · {1} warten", "{0} Dateien zuerst · {1} warten"],
    &["{0} archivo primero · {1} esperando", "{0} archivos primero · {1} esperando"],
    &["{0} fichier d'abord · {1} en attente", "{0} fichiers d'abord · {1} en attente"],
    &["{0} फ़ाइल पहले · {1} प्रतीक्षा में", "{0} फ़ाइलें पहले · {1} प्रतीक्षा में"],
    &["{0} berkas didahulukan · {1} menunggu"],
    &["{0} arquivo na frente · {1} esperando", "{0} arquivos na frente · {1} esperando"],
    &[
        "{0} файл первым · {1} в очереди",
        "{0} файла первыми · {1} в очереди",
        "{0} файлов первыми · {1} в очереди",
    ],
    &["{0} dosya önce · {1} bekliyor"],
    &["{0} tệp trước · {1} đang chờ"],
];

/// "12 files · 3.72 GB" — the whole torrent, nothing left out.
pub(super) const FILES_WHOLE: [&[&str]; 11] = [
    &["{0} file · {1}", "{0} files · {1}"],
    &["{0} ملفات · {1}"],
    &["{0} Datei · {1}", "{0} Dateien · {1}"],
    &["{0} archivo · {1}", "{0} archivos · {1}"],
    &["{0} fichier · {1}", "{0} fichiers · {1}"],
    &["{0} फ़ाइल · {1}", "{0} फ़ाइलें · {1}"],
    &["{0} berkas · {1}"],
    &["{0} arquivo · {1}", "{0} arquivos · {1}"],
    &["{0} файл · {1}", "{0} файла · {1}", "{0} файлов · {1}"],
    &["{0} dosya · {1}"],
    &["{0} tệp · {1}"],
];

/// "3 of 12 files · 1.44 GB of 3.72 GB" — a choice made inside it.
pub(super) const FILES_PART: [&[&str]; 11] = [
    &["{0} of {1} file · {2} of {3}", "{0} of {1} files · {2} of {3}"],
    &["{0} من {1} ملفات · {2} من {3}"],
    &["{0} von {1} Datei · {2} von {3}", "{0} von {1} Dateien · {2} von {3}"],
    &["{0} de {1} archivo · {2} de {3}", "{0} de {1} archivos · {2} de {3}"],
    &["{0} sur {1} fichier · {2} sur {3}", "{0} sur {1} fichiers · {2} sur {3}"],
    &["{1} में से {0} फ़ाइल · {3} में से {2}", "{1} में से {0} फ़ाइलें · {3} में से {2}"],
    &["{0} dari {1} berkas · {2} dari {3}"],
    &["{0} de {1} arquivo · {2} de {3}", "{0} de {1} arquivos · {2} de {3}"],
    &["{0} из {1} файла · {2} из {3}", "{0} из {1} файлов · {2} из {3}", "{0} из {1} файлов · {2} из {3}"],
    &["{1} dosyadan {0} · {3} içinde {2}"],
    &["{0} trên {1} tệp · {2} trên {3}"],
];
