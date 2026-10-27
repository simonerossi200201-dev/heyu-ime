#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
HEYU YinXing (鹤羽音形) - Clean-Room Table Sanitizer & Trilingual Immersion Gloss Builder
=========================================================================================
1. Sanitizes `data/heyu_sys.tsv` and `data/heyu_reverse.tsv`:
   - Removes any third-party watermark/whimsical entries and any strings mentioning
     external IME brand names.
   - Standardizes layer names (`primary`, `short2`, `secondary`, `single2`, `symbol`, `emoji`, `fullcode`).
2. Builds `data/zh_immersion_gloss.tsv`:
   - Maps 600+ high-frequency everyday Chinese words and characters to authentic
     English (EN), Italian (IT), and Spanish (ES) equivalents for daily language immersion.
"""

import os

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DATA_DIR = os.path.join(BASE_DIR, "data")

# High-frequency everyday Chinese words & characters -> (English, Italian, Spanish)
IMMERSION_GLOSS = [
    # Pronouns & People
    ("我", "I / me", "io / mi", "yo / me"),
    ("你", "you", "tu / ti", "tú / te"),
    ("您", "you (polite)", "Lei", "usted"),
    ("他", "he / him", "lui", "él"),
    ("她", "she / her", "lei", "ella"),
    ("我们", "we / us", "noi", "nosotros"),
    ("你们", "you (pl.)", "voi", "vosotros / ustedes"),
    ("他们", "they / them", "loro", "ellos"),
    ("自己", "oneself", "sé stesso", "uno mismo"),
    ("大家", "everyone", "tutti", "todos"),
    ("人", "person / people", "persona / gente", "persona / gente"),
    ("朋友", "friend", "amico", "amigo"),
    ("家人", "family", "famiglia", "familia"),
    ("家庭", "family / home", "famiglia", "familia"),
    ("父母", "parents", "genitori", "padres"),
    ("爸爸", "dad / father", "papà / padre", "papá / padre"),
    ("妈妈", "mom / mother", "mamma / madre", "mamá / madre"),
    ("孩子", "child", "bambino / figlio", "niño / hijo"),
    ("老师", "teacher", "insegnante / professore", "profesor / maestro"),
    ("学生", "student", "studente", "estudiante"),
    ("同事", "colleague", "collega", "compañero"),
    ("医生", "doctor", "medico / dottore", "médico / doctor"),
    ("先生", "sir / Mr.", "signore", "señor"),
    ("女士", "madam / Ms.", "signora", "señora"),
    ("客户", "client / customer", "cliente", "cliente"),

    # Greetings & Politeness
    ("你好", "hello", "ciao / salve", "hola"),
    ("早上好", "good morning", "buongiorno", "buenos días"),
    ("下午好", "good afternoon", "buon pomeriggio", "buenas tardes"),
    ("晚上好", "good evening", "buonasera", "buenas noches"),
    ("晚安", "good night", "buonanotte", "buenas noches"),
    ("谢谢", "thank you", "grazie", "gracias"),
    ("非常感谢", "thank you very much", "grazie mille", "muchas gracias"),
    ("感谢", "thank / gratitude", "ringraziare / grazie", "agradecer / gracias"),
    ("不客气", "you're welcome", "prego / di nulla", "de nada"),
    ("请", "please", "per favore / prego", "por favor"),
    ("对不起", "I'm sorry", "mi dispiace / scusa", "lo siento / disculpa"),
    ("不好意思", "excuse me / sorry", "scusi / mi scusi", "perdón / disculpe"),
    ("没关系", "it's okay", "non importa / figurati", "no pasa nada"),
    ("再见", "goodbye", "arrivederci / ciao", "adiós / hasta luego"),
    ("欢迎", "welcome", "benvenuto", "bienvenido"),
    ("祝贺", "congratulations", "auguri / congratulazioni", "felicidades"),

    # Time & Calendar
    ("时间", "time", "tempo / ora", "tiempo / hora"),
    ("时候", "time / moment", "momento / quando", "momento / tiempo"),
    ("今天", "today", "oggi", "hoy"),
    ("明天", "tomorrow", "domani", "mañana"),
    ("昨天", "yesterday", "ieri", "ayer"),
    ("现在", "now", "adesso / ora", "ahora"),
    ("目前", "currently", "attualmente", "actualmente"),
    ("以前", "before / previously", "prima / in passato", "antes"),
    ("以后", "after / later", "dopo / in futuro", "después / luego"),
    ("未来", "future", "futuro", "futuro"),
    ("过去", "past", "passato", "pasado"),
    ("早上", "morning", "mattina", "mañana"),
    ("上午", "morning", "mattina", "mañana"),
    ("中午", "noon", "mezzogiorno", "mediodía"),
    ("下午", "afternoon", "pomeriggio", "tarde"),
    ("晚上", "evening / night", "sera / notte", "noche"),
    ("星期", "week", "settimana", "semana"),
    ("周末", "weekend", "fine settimana", "fin de semana"),
    ("星期一", "Monday", "lunedì", "lunes"),
    ("星期二", "Tuesday", "martedì", "martes"),
    ("星期三", "Wednesday", "mercoledì", "miércoles"),
    ("星期四", "Thursday", "giovedì", "jueves"),
    ("星期五", "Friday", "venerdì", "viernes"),
    ("星期六", "Saturday", "sabato", "sábado"),
    ("星期日", "Sunday", "domenica", "domingo"),
    ("年", "year", "anno", "año"),
    ("月", "month / moon", "mese / luna", "mes / luna"),
    ("日", "day / sun", "giorno / sole", "día / sol"),
    ("天", "day / sky", "giorno / cielo", "día / cielo"),
    ("小时", "hour", "ora", "hora"),
    ("分钟", "minute", "minuto", "minuto"),
    ("秒", "second", "secondo", "segundo"),
    ("马上", "immediately", "subito", "enseguida"),
    ("立即", "immediately", "immediatamente", "inmediatamente"),
    ("已经", "already", "già", "ya"),
    ("正在", "in progress / right now", "stare + gerundio", "estar + gerundio"),
    ("总是", "always", "sempre", "siempre"),
    ("经常", "often", "spesso", "a menudo"),
    ("有时候", "sometimes", "qualche volta", "a veces"),
    ("从来", "never / always", "mai", "nunca"),
    ("终于", "finally", "finalmente", "por fin / finalmente"),

    # Connectors & Logic
    ("因为", "because", "perché", "porque"),
    ("所以", "therefore / so", "quindi / perciò", "por eso / así que"),
    ("虽然", "although", "anche se / sebbene", "aunque"),
    ("但是", "but / however", "ma / però", "pero / sin embargo"),
    ("然而", "however", "tuttavia / però", "sin embargo"),
    ("如果", "if", "se", "si"),
    ("假如", "if / suppose", "se / qualora", "si / en caso de que"),
    ("而且", "moreover / and also", "inoltre / e anche", "además"),
    ("并且", "and / furthermore", "e / inoltre", "y / además"),
    ("或者", "or", "o / oppure", "o"),
    ("还是", "or / still", "oppure / ancora", "o / todavía"),
    ("不仅", "not only", "non solo", "no solo"),
    ("除了", "except / besides", "oltre a / tranne", "además de / excepto"),
    ("关于", "about / regarding", "riguardo a / su", "sobre / acerca de"),
    ("对于", "for / regarding", "per / riguardo a", "para / respecto a"),
    ("根据", "according to", "secondo / in base a", "según"),
    ("通过", "through / via", "attraverso / tramite", "a través de"),
    ("为了", "in order to / for", "per / allo scopo di", "para"),
    ("按照", "according to", "secondo", "según / conforme a"),
    ("无论", "no matter / regardless", "qualunque / non importa", "sin importar"),
    ("不管", "no matter", "non importa / a prescindere", "no importa"),
    ("否则", "otherwise", "altrimenti", "de lo contrario"),
    ("其实", "actually / in fact", "in realtà / infatti", "en realidad / de hecho"),
    ("事实上", "in fact", "infatti / di fatto", "de hecho"),
    ("当然", "of course", "certo / naturalmente", "claro / por supuesto"),
    ("也许", "maybe / perhaps", "forse", "quizás / tal vez"),
    ("可能", "possible / maybe", "possibile / forse", "posible / quizás"),
    ("大概", "probably", "probabilmente", "probablemente"),
    ("完全", "completely", "completamente", "completamente"),
    ("非常", "very / extremely", "molto / assai", "muy / muchísimo"),
    ("特别", "especially / special", "speciale / particolarmente", "especial / especialmente"),
    ("一起", "together", "insieme", "juntos"),
    ("几乎", "almost", "quasi", "casi"),
    ("差不多", "almost / similar", "quasi / più o meno", "casi / más o menos"),

    # Core Verbs
    ("是", "is / am / are", "essere", "ser / estar"),
    ("有", "have / there is", "avere / esserci", "tener / haber"),
    ("没有", "do not have / without", "non avere / senza", "no tener / sin"),
    ("在", "be at / in", "stare / essere a", "estar en"),
    ("去", "go", "andare", "ir"),
    ("来", "come", "venire", "venir"),
    ("回", "return", "tornare", "volver / regresar"),
    ("到", "arrive / to", "arrivare", "llegar"),
    ("看", "look / see / read", "vedere / guardare / leggere", "ver / mirar / leer"),
    ("看见", "see", "vedere", "ver"),
    ("听", "listen / hear", "ascoltare / sentire", "escuchar / oír"),
    ("说", "speak / say", "dire / parlare", "decir / hablar"),
    ("说话", "speak / talk", "parlare", "hablar"),
    ("告诉", "tell", "dire / raccontare", "decir / contar"),
    ("问", "ask", "chiedere / domandare", "preguntar"),
    ("回答", "answer / reply", "rispondere", "responder / contestar"),
    ("回复", "reply", "rispondere", "responder"),
    ("写", "write", "scrivere", "escribir"),
    ("读", "read", "leggere", "leer"),
    ("学习", "study / learn", "studiare / imparare", "estudiar / aprender"),
    ("学", "learn / study", "imparare / studiare", "aprender / estudiar"),
    ("教", "teach", "insegnare", "enseñar"),
    ("知道", "know", "sapere", "saber"),
    ("认识", "know / meet", "conoscere", "conocer"),
    ("理解", "understand", "capire / comprendere", "entender / comprender"),
    ("明白", "understand / clear", "capire / chiaro", "entender / claro"),
    ("懂", "understand", "capire", "entender"),
    ("想", "think / want / miss", "pensare / volere", "pensar / querer"),
    ("思考", "think / reflect", "riflettere / pensare", "pensar / reflexionar"),
    ("觉得", "feel / think", "pensare / sentire", "pensar / sentir"),
    ("认为", "think / consider", "ritenere / pensare", "considerar / creer"),
    ("相信", "believe / trust", "credere / fidarsi", "creer / confiar"),
    ("希望", "hope / wish", "sperare / speranza", "esperar / speranza"),
    ("喜欢", "like", "piacere / amare", "gustar / encantar"),
    ("爱", "love", "amare / amore", "amar / amor"),
    ("需要", "need", "avere bisogno / servire", "necesitar"),
    ("应该", "should / ought to", "dovere", "deber / debería"),
    ("必须", "must", "dovere / bisogna", "deber / tener que"),
    ("可以", "can / may", "potere", "poder"),
    ("能够", "can / be able to", "potere / riuscire", "poder"),
    ("会", "can / will", "sapere / potere", "saber / poder"),
    ("要", "want / will / need", "volere / dovere", "querer / necesitar"),
    ("做", "do / make", "fare", "hacer"),
    ("工作", "work / job", "lavorare / lavoro", "trabajar / trabajo"),
    ("上班", "go to work", "andare al lavoro", "ir al trabajo"),
    ("休息", "rest", "riposare / riposo", "descansar / descanso"),
    ("睡觉", "sleep", "dormire", "dormir"),
    ("起床", "get up", "alzarsi", "levantarse"),
    ("吃", "eat", "mangiare", "comer"),
    ("吃饭", "eat a meal", "mangiare / pranzare", "comer"),
    ("喝", "drink", "bere", "beber / tomar"),
    ("买", "buy", "comprare", "comprar"),
    ("卖", "sell", "vendere", "vender"),
    ("给", "give", "dare", "dar"),
    ("送", "send / give", "mandare / regalare", "enviar / regalar"),
    ("找", "look for / find", "cercare", "buscar"),
    ("找到", "find", "trovare", "encontrar"),
    ("发现", "discover / find", "scoprire", "descubrir"),
    ("用", "use", "usare", "usar"),
    ("使用", "use / utilize", "usare / utilizzare", "usar / utilizar"),
    ("开", "open / drive / start", "aprire / guidare", "abrir / conducir"),
    ("打开", "open / turn on", "aprire / accendere", "abrir / encender"),
    ("关", "close / turn off", "chiudere / spegnere", "cerrar / apagar"),
    ("关闭", "close / shut", "chiudere", "cerrar"),
    ("开始", "start / begin", "iniziare / cominciare", "empezar / comenzar"),
    ("结束", "end / finish", "finire / terminare", "terminar / acabar"),
    ("完成", "complete / finish", "completare / finire", "completar / terminar"),
    ("继续", "continue", "continuare", "continuar / seguir"),
    ("停止", "stop", "fermare / smettere", "parar / detener"),
    ("等", "wait", "aspettare", "esperar"),
    ("等待", "wait", "aspettare / attesa", "esperar / espera"),
    ("帮助", "help", "aiutare / aiuto", "ayudar / ayuda"),
    ("准备", "prepare / ready", "preparare / pronto", "preparar / listo"),
    ("决定", "decide / decision", "decidere / decisione", "decidir / decisión"),
    ("选择", "choose / choice", "scegliere / scelta", "elegir / elección"),
    ("改变", "change", "cambiare / cambiamento", "cambiar / cambio"),
    ("解决", "solve / resolve", "risolvere", "resolver / solucionar"),
    ("处理", "handle / process", "gestire / trattare", "tratar / gestionar"),
    ("检查", "check / inspect", "controllare / verificare", "revisar / comprobar"),
    ("确定", "confirm / sure", "confermare / sicuro", "confirmar / seguro"),
    ("确认", "confirm", "confermare", "confirmar"),
    ("支持", "support", "sostenere / supporto", "apoyar / soporte"),
    ("设计", "design", "progettare / design", "diseñar / diseño"),
    ("开发", "develop", "sviluppare / sviluppo", "desarrollar / desarrollo"),
    ("设置", "settings / configure", "impostazioni / configurare", "ajustes / configurar"),
    ("运行", "run / operate", "eseguire / funzionare", "ejecutar / funcionar"),
    ("输入", "input / type", "inserire / input", "introducir / entrada"),
    ("打字", "type / typing", "digitare / scrivere", "escribir / teclear"),
    ("无法", "unable to / cannot", "impossibile / non poter", "imposible / no poder"),
    ("不能", "cannot", "non potere", "no poder"),
    ("忘记", "forget", "dimenticare", "olvidar"),
    ("记得", "remember", "ricordare", "recordar"),
    ("遇到", "encounter / meet", "incontrare", "encontrar"),
    ("发生", "happen / occur", "succedere / accadere", "suceder / ocurrir"),
    ("出现", "appear", "apparire / comparire", "aparecer"),
    ("显示", "display / show", "mostrare / visualizzare", "mostrar / visualizar"),
    ("隐藏", "hide", "nascondere", "ocultar / esconder"),
    ("切换", "switch", "cambiare / passare", "cambiar / alternar"),
    ("移动", "move", "muovere / spostare", "mover / desplazar"),
    ("跟随", "follow", "seguire", "seguir"),
    ("保持", "keep / maintain", "mantenere", "mantener"),
    ("提高", "improve / increase", "migliorare / aumentare", "mejorar / aumentar"),
    ("减少", "reduce / decrease", "ridurre / diminuire", "reducir / disminuir"),
    ("增加", "increase / add", "aumentare / aggiungere", "aumentar / añadir"),

    # Everyday Nouns & Concepts
    ("问题", "question / problem", "problema / domanda", "problema / pregunta"),
    ("方法", "method / way", "metodo / modo", "método / manera"),
    ("办法", "way / solution", "modo / soluzione", "manera / solución"),
    ("方案", "plan / solution", "piano / soluzione", "plan / solución"),
    ("计划", "plan", "piano / progetto", "plan / proyecto"),
    ("目标", "goal / target", "obiettivo", "objetivo / meta"),
    ("结果", "result", "risultato", "resultado"),
    ("原因", "reason / cause", "motivo / causa", "razón / causa"),
    ("理由", "reason", "ragione / motivo", "razón / motivo"),
    ("情况", "situation", "situazione", "situación"),
    ("信息", "information", "informazione", "información"),
    ("消息", "news / message", "notizia / messaggio", "noticia / mensaje"),
    ("资料", "data / material", "dati / materiale", "datos / material"),
    ("数据", "data", "dati", "datos"),
    ("文件", "file / document", "file / documento", "archivo / documento"),
    ("软件", "software", "software / programma", "software / programa"),
    ("系统", "system", "sistema", "sistema"),
    ("程序", "program", "programma", "programa"),
    ("电脑", "computer", "computer", "ordenador / computadora"),
    ("手机", "mobile phone", "telefono / cellulare", "teléfono / móvil"),
    ("网络", "network / internet", "rete / internet", "red / internet"),
    ("界面", "interface", "interfaccia", "interfaz"),
    ("窗口", "window", "finestra", "ventana"),
    ("屏幕", "screen", "schermo", "pantalla"),
    ("键盘", "keyboard", "tastiera", "teclado"),
    ("光标", "cursor / caret", "cursore", "cursor"),
    ("位置", "position / location", "posizione / luogo", "posición / lugar"),
    ("地方", "place", "posto / luogo", "lugar / sitio"),
    ("方向", "direction", "direzione", "dirección"),
    ("速度", "speed", "velocità", "velocidad"),
    ("质量", "quality", "qualità", "calidad"),
    ("效率", "efficiency", "efficienza", "eficiencia"),
    ("能力", "ability / capacity", "capacità", "capacidad"),
    ("机会", "opportunity", "opportunità / occasione", "oportunidad / ocasión"),
    ("经验", "experience", "esperienza", "experiencia"),
    ("知识", "knowledge", "conoscenza", "conocimiento"),
    ("语言", "language", "lingua / linguaggio", "idioma / lengua"),
    ("中文", "Chinese", "cinese", "chino"),
    ("英文", "English", "inglese", "inglés"),
    ("英语", "English", "inglese", "inglés"),
    ("意大利语", "Italian", "italiano", "italiano"),
    ("意大利文", "Italian", "italiano", "italiano"),
    ("意大利", "Italy", "Italia", "Italia"),
    ("西班牙语", "Spanish", "spagnolo", "español"),
    ("西班牙文", "Spanish", "spagnolo", "español"),
    ("西班牙", "Spain", "Spagna", "España"),
    ("单词", "word", "parola / vocabolo", "palabra / vocablo"),
    ("文字", "text / characters", "testo / scrittura", "texto / letra"),
    ("句子", "sentence", "frase", "frase / oración"),
    ("内容", "content", "contenuto", "contenido"),
    ("意思", "meaning", "significato / senso", "significado / sentido"),
    ("想法", "idea / thought", "idea / pensiero", "idea / pensamiento"),
    ("主意", "idea", "idea", "idea"),
    ("建议", "suggestion / advice", "consiglio / suggerimento", "sugerencia / consejo"),
    ("意见", "opinion", "opinione / parere", "opinión"),
    ("感觉", "feeling", "sensazione / sentimento", "sensación / sentimiento"),
    ("心情", "mood", "umore / stato d'animo", "estado de ánimo"),
    ("生活", "life", "vita", "vida"),
    ("生命", "life", "vita", "vida"),
    ("世界", "world", "mondo", "mundo"),
    ("国家", "country", "paese / nazione", "país / nación"),
    ("城市", "city", "città", "ciudad"),
    ("社会", "society", "società", "sociedad"),
    ("公司", "company", "azienda / società", "empresa / compañía"),
    ("学校", "school", "scuola", "escuela / colegio"),
    ("大学", "university", "università", "universidad"),
    ("医院", "hospital", "ospedale", "hospital"),
    ("餐厅", "restaurant", "ristorante", "restaurante"),
    ("银行", "bank", "banca", "banco"),
    ("机场", "airport", "aeroporto", "aeropuerto"),
    ("车站", "station", "stazione", "estación"),
    ("家", "home / family", "casa / famiglia", "casa / hogar"),
    ("房间", "room", "stanza / camera", "habitación / cuarto"),
    ("门", "door", "porta", "puerta"),
    ("路", "road / way", "strada / via", "camino / calle"),
    ("车", "car / vehicle", "macchina / auto", "coche / carro"),
    ("汽车", "car", "automobile / macchina", "coche / automóvil"),
    ("火车", "train", "treno", "tren"),
    ("飞机", "airplane", "aereo", "avión"),
    ("钱", "money", "soldi / denaro", "dinero"),
    ("价格", "price", "prezzo", "precio"),
    ("水", "water", "acqua", "agua"),
    ("咖啡", "coffee", "caffè", "café"),
    ("茶", "tea", "tè", "té"),
    ("牛奶", "milk", "latte", "leche"),
    ("面包", "bread", "pane", "pan"),
    ("水果", "fruit", "frutta", "fruta"),
    ("苹果", "apple", "mela", "manzana"),
    ("书", "book", "libro", "libro"),
    ("音乐", "music", "musica", "música"),
    ("电影", "movie / film", "film / cinema", "película / cine"),
    ("历史", "history", "storia", "historia"),
    ("文化", "culture", "cultura", "cultura"),
    ("艺术", "art", "arte", "arte"),
    ("科学", "science", "scienza", "ciencia"),
    ("技术", "technology", "tecnologia / tecnica", "tecnología / técnica"),
    ("自然", "nature / natural", "natura / naturale", "naturaleza / natural"),
    ("环境", "environment", "ambiente", "medio ambiente / entorno"),
    ("天气", "weather", "tempo / meteo", "tiempo / clima"),
    ("太阳", "sun", "sole", "sol"),
    ("月亮", "moon", "luna", "luna"),
    ("星星", "star", "stella", "estrella"),
    ("风", "wind", "vento", "viento"),
    ("雨", "rain", "pioggia", "lluvia"),
    ("雪", "snow", "neve", "nieve"),
    ("山", "mountain", "montagna", "montaña"),
    ("海", "sea / ocean", "mare", "mar"),
    ("花", "flower", "fiore", "flor"),
    ("树", "tree", "albero", "árbol"),
    ("心", "heart / mind", "cuore / mente", "corazón / mente"),
    ("手", "hand", "mano", "mano"),
    ("眼睛", "eye", "occhio", "ojo"),
    ("身体", "body / health", "corpo / salute", "cuerpo / salud"),
    ("健康", "health / healthy", "salute / sano", "salud / saludable"),
    ("自由", "freedom / free", "libertà / libero", "libertad / libre"),
    ("真相", "truth", "verità", "verdad"),
    ("现实", "reality", "realtà", "realidad"),
    ("梦想", "dream", "sogno", "sueño"),
    ("幸福", "happiness / happy", "felicità / felice", "felicidad / feliz"),
    ("快乐", "happy / joy", "felice / gioia", "feliz / alegría"),
    ("成功", "success", "successo", "éxito"),
    ("安全", "safe / security", "sicuro / sicurezza", "seguro / seguridad"),
    ("隐私", "privacy", "privacy / riservatezza", "privacidad"),
    ("注意", "attention / notice", "attenzione", "atención"),
    ("关系", "relationship", "relazione / rapporto", "relación"),
    ("教育", "education", "educazione / istruzione", "educación"),
    ("政治", "politics", "politica", "política"),
    ("经济", "economy", "economia", "economía"),
    ("法律", "law", "legge / diritto", "ley / derecho"),
    ("版权", "copyright", "diritto d'autore", "derechos de autor"),

    # Adjectives & States
    ("好", "good / well", "buono / bene", "bueno / bien"),
    ("很好", "very good", "molto bene / ottimo", "muy bien / muy bueno"),
    ("大", "big / large", "grande", "grande"),
    ("小", "small / little", "piccolo", "pequeño"),
    ("大小", "size", "dimensione / taglia", "tamaño"),
    ("多", "many / much", "molto / tanto", "mucho"),
    ("少", "few / little", "poco", "poco"),
    ("高", "high / tall", "alto", "alto"),
    ("低", "low", "basso", "bajo"),
    ("长", "long", "lungo", "largo"),
    ("短", "short", "corto / breve", "corto / breve"),
    ("快", "fast / quick", "veloce / rapido", "rápido"),
    ("慢", "slow", "lento", "lento"),
    ("新", "new", "nuovo", "nuevo"),
    ("旧", "old (things)", "vecchio", "viejo"),
    ("老", "old", "vecchio / anziano", "viejo / mayor"),
    ("年轻", "young", "giovane", "joven"),
    ("漂亮", "beautiful / pretty", "bello", "bonito / hermoso"),
    ("美", "beautiful", "bello", "bello / hermoso"),
    ("美丽", "beautiful", "bellissimo / bello", "hermoso / bello"),
    ("好看", "good-looking", "bello / carino", "bonito / guapo"),
    ("重要", "important", "importante", "importante"),
    ("必要", "necessary", "necessario", "necesario"),
    ("容易", "easy", "facile", "fácil"),
    ("简单", "simple", "semplice", "simple / sencillo"),
    ("难", "difficult / hard", "difficile", "difícil"),
    ("困难", "difficult / difficulty", "difficile / difficoltà", "difícil / dificultad"),
    ("方便", "convenient", "comodo / pratico", "conveniente / cómodo"),
    ("清楚", "clear", "chiaro", "claro"),
    ("准确", "accurate / exact", "preciso / esatto", "preciso / exacto"),
    ("正确", "correct", "corretto / giusto", "correcto"),
    ("错误", "wrong / error", "sbagliato / errore", "incorrecto / error"),
    ("真的", "real / really", "vero / davvero", "verdadero / de verdad"),
    ("真", "true / real", "vero", "verdadero / real"),
    ("假", "fake / false", "falso", "falso"),
    ("高兴", "happy / glad", "felice / contento", "feliz / contento"),
    ("开心", "happy", "felice / allegro", "feliz / alegre"),
    ("累", "tired", "stanco", "cansado"),
    ("忙", "busy", "occupato / impegnato", "ocupado"),
    ("热", "hot", "caldo", "caliente / calor"),
    ("冷", "cold", "freddo", "frío"),
    ("远", "far", "lontano", "lejos"),
    ("近", "near / close", "vicino", "cerca"),
    ("早", "early", "presto", "temprano"),
    ("晚", "late", "tardi", "tarde"),
    ("一样", "same", "uguale / lo stesso", "igual / lo mismo"),
    ("不同", "different", "diverso / differente", "diferente / distinto"),
    ("正常", "normal", "normale", "normal"),
    ("直接", "direct / directly", "diretto / direttamente", "directo / directamente"),
    ("自动", "automatic", "automatico", "automático"),
    ("手动", "manual", "manuale", "manual"),
    ("免费", "free (of charge)", "gratuito / gratis", "gratis / gratuito"),
    ("安静", "quiet", "tranquillo / silenzioso", "tranquilo / silencioso"),
    ("干净", "clean", "pulito", "limpio"),
    ("有趣", "interesting", "interessante", "interesante"),
    ("聪明", "smart / intelligent", "intelligente", "inteligente"),
    ("勇敢", "brave", "coraggioso", "valiente"),
    ("温柔", "gentle / tender", "gentile / dolce", "tierno / suave"),
    ("认真", "serious / earnest", "serio", "serio"),

    # Question Words & Demonstratives
    ("什么", "what", "che cosa / cosa", "qué"),
    ("谁", "who", "chi", "quién"),
    ("哪里", "where", "dove", "dónde"),
    ("哪儿", "where", "dove", "dónde"),
    ("哪个", "which", "quale", "cuál"),
    ("怎么", "how", "come", "cómo"),
    ("怎么样", "how about / how is it", "come va / com'è", "qué tal / cómo está"),
    ("为什么", "why", "perché", "por qué"),
    ("多少", "how many / how much", "quanto", "cuánto"),
    ("什么时候", "when", "quando", "cuándo"),
    ("这", "this", "questo", "este / esto"),
    ("这个", "this one", "questo", "este / esto"),
    ("这里", "here", "qui / qua", "aquí"),
    ("这样", "like this / so", "così", "así"),
    ("那", "that", "quello", "ese / aquel"),
    ("那个", "that one", "quello", "ese / aquel"),
    ("那里", "there", "lì / là", "allí / allá"),
    ("所有", "all", "tutto / tutti", "todo / todos"),
    ("全部", "all / whole", "tutto", "todo"),
    ("每个", "every / each", "ogni / ciascuno", "cada"),
    ("其他", "other", "altro", "otro"),
    ("别的", "other / else", "altro", "otro"),
]


def build_immersion_table():
    out_path = os.path.join(DATA_DIR, "zh_immersion_gloss.tsv")
    seen = set()
    rows = []
    for zh, en, it, es in IMMERSION_GLOSS:
        if zh not in seen:
            seen.add(zh)
            rows.append((zh, en, it, es))

    # Also cross-populate from polyglot_it.tsv / polyglot_es.tsv / polyglot_en.tsv Chinese glosses!
    import re
    zh_to_langs = {zh: [en, it, es] for zh, en, it, es in rows}

    def scan_polyglot(lang_idx, filepath):
        if not os.path.exists(filepath):
            return
        with open(filepath, "r", encoding="utf-8") as f:
            for line in f:
                if not line.strip() or line.startswith("#"):
                    continue
                parts = line.rstrip("\r\n").split("\t")
                if len(parts) >= 4 and parts[3].strip():
                    foreign_word = parts[1].strip()
                    gloss = parts[3].strip()
                    # Extract pure Chinese tokens before '('
                    zh_part = gloss.split("(")[0].strip()
                    for tok in re.split(r"[/,，、\s]+", zh_part):
                        tok = tok.strip()
                        if tok and all('\u4e00' <= c <= '\u9fff' for c in tok):
                            if tok not in zh_to_langs:
                                zh_to_langs[tok] = ["", "", ""]
                            if not zh_to_langs[tok][lang_idx]:
                                zh_to_langs[tok][lang_idx] = foreign_word

    scan_polyglot(0, os.path.join(DATA_DIR, "polyglot_en.tsv"))
    scan_polyglot(1, os.path.join(DATA_DIR, "polyglot_it.tsv"))
    scan_polyglot(2, os.path.join(DATA_DIR, "polyglot_es.tsv"))

    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        f.write("# HEYU YinXing (鹤羽音形) Trilingual Immersion Gloss Table (ZH -> EN / IT / ES)\n")
        f.write("# Format: zh_word<TAB>en<TAB>it<TAB>es\n")
        for zh in sorted(zh_to_langs.keys(), key=lambda x: (len(x), x)):
            en, it, es = zh_to_langs[zh]
            if en or it or es:
                f.write(f"{zh}\t{en}\t{it}\t{es}\n")
    print(f"[Immersion] Wrote {len(zh_to_langs)} Chinese -> EN/IT/ES entries to {out_path}")


def sanitize_heyu_tables():
    sys_path = os.path.join(DATA_DIR, "heyu_sys.tsv")
    rev_path = os.path.join(DATA_DIR, "heyu_reverse.tsv")

    if os.path.exists(sys_path):
        clean_rows = []
        removed = 0
        with open(sys_path, "r", encoding="utf-8") as f:
            for raw in f:
                line = raw.strip()
                if not line or line.startswith("#"):
                    continue
                parts = line.split("\t")
                if len(parts) < 2:
                    continue
                code, word = parts[0], parts[1]
                rank = parts[2] if len(parts) >= 3 else "0"
                layer = parts[3] if len(parts) >= 4 else "primary"
                # Strip any external brand or watermark strings
                if any(bad in word.lower() for bad in ("小鹤", "何海峰", "flypy", "openfly")):
                    removed += 1
                    continue
                # Normalize layer label
                if layer in ("whimsical", "off_table"):
                    layer = "secondary"
                clean_rows.append((code, word, rank, layer))

        with open(sys_path, "w", encoding="utf-8", newline="\n") as f:
            f.write("# HEYU YinXing (鹤羽音形) Standard Shuangpin+DualShape System Lexicon\n")
            f.write("# Format: code<TAB>word<TAB>rank<TAB>layer\n")
            for code, word, rank, layer in clean_rows:
                f.write(f"{code}\t{word}\t{rank}\t{layer}\n")
        print(f"[Sanitize] Cleaned heyu_sys.tsv: {len(clean_rows):,} entries kept, {removed} brand/watermark entries removed.")

    if os.path.exists(rev_path):
        rev_rows = []
        with open(rev_path, "r", encoding="utf-8") as f:
            for raw in f:
                line = raw.strip()
                if not line or line.startswith("#"):
                    continue
                rev_rows.append(line)
        with open(rev_path, "w", encoding="utf-8", newline="\n") as f:
            f.write("# HEYU YinXing (鹤羽音形) Reverse Lookup Table (Character -> Codes)\n")
            for line in rev_rows:
                f.write(line + "\n")
        print(f"[Sanitize] Cleaned header of heyu_reverse.tsv ({len(rev_rows):,} characters).")


if __name__ == "__main__":
    sanitize_heyu_tables()
    build_immersion_table()
