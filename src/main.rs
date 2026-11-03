use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::{Query, State},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use chrono::{Duration, Utc};
use chrono_tz::Asia::Yekaterinburg;
use serde::Deserialize;
use sqlx::PgPool; 
use std::collections::HashMap;
use tokio_cron_scheduler::{Job, JobScheduler};

// ====== КОНСТАНТЫ ======
const TELEGRAM_BOT_TOKEN: &str = "9026262070:AAF7Qb7oIrKpexDtxxK6nxKWFNai9K3VGOI"; 
const TELEGRAM_CHAT_ID: &str = "359039555";     
const CMC_API_KEY: &str = "c15605f5e9ae46fc82ab48c750453fef";

const BOTGATE_API_KEY: &str = "bg_live_93075222887ee73daae79ee16fe4ff1b";
const BOTGATE_PUBLIC_BOT_ID: &str = "bot_CoWfsLW0vN14";
const BOTGATE_BASE_URL: &str = "https://bot-gate.ru";

// ====== СТРУКТУРЫ ДЛЯ API ======
#[derive(Deserialize, Debug)]
struct CoinPaprikaResponse {
    quotes: Quotes,
}
#[derive(Deserialize, Debug)]
struct Quotes {
    #[serde(rename = "USD")]
    usd: QuoteData,
}
#[derive(Deserialize, Debug)]
struct CmcResponse {
    data: HashMap<String, CmcCoin>,
}
#[derive(Deserialize, Debug)]
struct CmcCoin {
    quote: CmcQuote,
}
#[derive(Deserialize, Debug)]
struct CmcQuote {
    #[serde(rename = "USD")]
    usd: QuoteData,
}
#[derive(Deserialize, Debug)]
struct QuoteData {
    price: f64,
    volume_24h: f64,
}

// ====== СТРУКТУРЫ ДЛЯ БД И ОТОБРАЖЕНИЯ ======
#[derive(sqlx::FromRow, Clone)]
struct Snapshot {
    source: String,
    price: f64,
    volume_24h: f64,
    fetched_at: String,
}

struct MainRow {
    source: String,
    price: String,
    volume: String,
    date: String,
    time: String,
    spread: String,
    volatility: String,
    volume_change: String,
    trend_class: String,
}

#[derive(sqlx::FromRow)]
struct HistoryRow {
    source: String,
    price: f64,
    volume_24h: f64,
    fetched_at: String,
}
struct HistoryRowFormatted {
    source: String,
    price: String,
    volume: String,
    date: String,
    time: String,
}

// ====== ШАБЛОНЫ ======
#[derive(Template, WebTemplate)]
#[template(path = "index.html")]
struct IndexTemplate {
    rows: Vec<MainRow>,
}

#[derive(Template, WebTemplate)]
#[template(path = "table.html")]
struct TableTemplate {
    rows: Vec<MainRow>,
}

#[derive(Template, WebTemplate)]
#[template(path = "history.html")]
struct HistoryTemplate {
    rows: Vec<HistoryRowFormatted>,
    current_period: String,
    chart_data_cp: String,
    chart_data_cmc: String,
}

#[derive(Deserialize)]
struct HistoryQuery {
    period: Option<String>,
}

// ====== ХЕЛПЕРЫ ======
fn format_volume(v: f64) -> String {
    if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if v >= 1_000.0 {
        format!("{:.1}K", v / 1_000.0)
    } else {
        format!("{:.0}", v)
    }
}

fn get_local_parts(iso_str: &str) -> (String, String) {
    if let Ok(utc_dt) = chrono::DateTime::parse_from_rfc3339(iso_str) {
        let local_dt = utc_dt.with_timezone(&Yekaterinburg);
        (
            local_dt.format("%d.%m.%Y").to_string(),
            local_dt.format("%H:%M:%S").to_string(),
        )
    } else {
        ("".to_string(), "".to_string())
    }
}

// ====== ОТПРАВКА В TELEGRAM ЧЕРЕЗ BOTGATE ======
async fn send_telegram(text: &str) -> Result<(), String> {
    let url = format!("{}/api/v1/bots/{}/sendMessage", BOTGATE_BASE_URL, BOTGATE_PUBLIC_BOT_ID);
    let client = reqwest::Client::new();
    let res = client.post(&url)
        .header("Authorization", format!("Bearer {}", BOTGATE_API_KEY))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({ "chat_id": TELEGRAM_CHAT_ID, "text": text, "parse_mode": "HTML" }))
        .send().await;

    match res {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() { Ok(()) } else {
                let err_body = resp.text().await.unwrap_or_default();
                Err(format!("HTTP {}: {}", status, err_body))
            }
        }
        Err(e) => Err(format!("Network error: {}", e)),
    }
}

// ====== ЛОГИКА СБОРА ДАННЫХ ======
async fn fetch_and_store(pool: &PgPool) {
    let client = reqwest::Client::new();
    let now = Utc::now().to_rfc3339();

    let cp_req = client.get("https://api.coinpaprika.com/v1/tickers/dcr-decred").header("User-Agent", "DCR-Rust-Monitor").send();
    let cmc_req = client.get("https://pro-api.coinmarketcap.com/v1/cryptocurrency/quotes/latest?id=1168&convert=USD").header("X-CMC_PRO_API_KEY", CMC_API_KEY).send();

    let (cp_res, cmc_res) = tokio::join!(cp_req, cmc_req);
    let mut current_prices = HashMap::new();

    if let Ok(resp) = cp_res {
        if let Ok(data) = resp.json::<CoinPaprikaResponse>().await {
            let p = data.quotes.usd.price;
            let v = data.quotes.usd.volume_24h;
            current_prices.insert("CoinPaprika", p);
            let _ = sqlx::query("INSERT INTO snapshots (source, price, volume_24h, fetched_at) VALUES ($1, $2, $3, $4)")
                .bind("CoinPaprika").bind(p).bind(v).bind(&now).execute(pool).await;
        }
    }

    if let Ok(resp) = cmc_res {
        if let Ok(data) = resp.json::<CmcResponse>().await {
            if let Some(coin) = data.data.get("1168") {
                let p = coin.quote.usd.price;
                let v = coin.quote.usd.volume_24h;
                current_prices.insert("CoinMarketCap", p);
                let _ = sqlx::query("INSERT INTO snapshots (source, price, volume_24h, fetched_at) VALUES ($1, $2, $3, $4)")
                    .bind("CoinMarketCap").bind(p).bind(v).bind(&now).execute(pool).await;
            }
        }
    }

    if let Some(&cp_price) = current_prices.get("CoinPaprika") {
        if let Ok(Some(prev)) = sqlx::query_scalar::<_, f64>("SELECT price FROM snapshots WHERE source = 'CoinPaprika' ORDER BY id DESC LIMIT 1 OFFSET 1")
            .fetch_optional(pool).await 
        {
            let change = ((cp_price - prev) / prev) * 100.0;
            if change.abs() >= 2.0 {
                let arrow = if change > 0.0 { "📈" } else { "📉" };
                let _ = send_telegram(&format!("{} <b>DCR скачок!</b>\nЦена: ${:.2} ({:+.2}%)\nБыло: ${:.2}", arrow, cp_price, change, prev)).await;
            }
        }
    }
}

async fn get_main_rows(pool: &PgPool) -> Vec<MainRow> {
    let mut rows = Vec::new();
    let sources = ["CoinPaprika", "CoinMarketCap"];
    let mut prices = HashMap::new();

    let hour_ago = (Utc::now() - Duration::hours(1)).to_rfc3339();

    for source in sources {
        let latest = sqlx::query_as::<_, Snapshot>(
            "SELECT source, price, volume_24h, fetched_at FROM snapshots WHERE source = $1 ORDER BY id DESC LIMIT 1"
        ).bind(source).fetch_optional(pool).await.unwrap_or(None);

        if let Some(snap) = latest {
            prices.insert(source, snap.price);
            
            let price_hour_ago: Option<f64> = sqlx::query_scalar(
                "SELECT price FROM snapshots WHERE source = $1 AND fetched_at <= $2 ORDER BY fetched_at DESC LIMIT 1"
            ).bind(source).bind(&hour_ago).fetch_optional(pool).await.unwrap_or(None);

            let trend_class = match price_hour_ago {
                Some(old_price) if snap.price > old_price => "row-up",
                Some(old_price) if snap.price < old_price => "row-down",
                _ => "row-neutral",
            };

            let recent_prices: Vec<f64> = sqlx::query_scalar("SELECT price FROM snapshots WHERE source = $1 ORDER BY id DESC LIMIT 10")
                .bind(source).fetch_all(pool).await.unwrap_or_default();
            
            let volatility = if recent_prices.len() > 1 {
                let mean = recent_prices.iter().sum::<f64>() / recent_prices.len() as f64;
                let variance = recent_prices.iter().map(|p| (p - mean).powi(2)).sum::<f64>() / recent_prices.len() as f64;
                format!("{:.2}%", (variance.sqrt() / mean) * 100.0)
            } else {
                "-".to_string()
            };

            let prev_vol = sqlx::query_scalar::<_, f64>("SELECT volume_24h FROM snapshots WHERE source = $1 ORDER BY id DESC LIMIT 1 OFFSET 1")
                .bind(source).fetch_optional(pool).await.unwrap_or(None);
            
            let vol_change = if let Some(pv) = prev_vol {
                if pv > 0.0 {
                    let change = ((snap.volume_24h - pv) / pv) * 100.0;
                    format!("{:+.1}%", change)
                } else { "-".to_string() }
            } else { "-".to_string() };

            let (date, time) = get_local_parts(&snap.fetched_at);
            rows.push(MainRow {
                source: snap.source,
                price: format!("{:.2}", snap.price),
                volume: format_volume(snap.volume_24h),
                date,
                time,
                spread: "-".to_string(),
                volatility,
                volume_change: vol_change,
                trend_class: trend_class.to_string(),
            });
        }
    }

    if let (Some(&cp), Some(&cmc)) = (prices.get("CoinPaprika"), prices.get("CoinMarketCap")) {
        let spread = ((cp - cmc) / cmc * 100.0).abs();
        for row in &mut rows {
            row.spread = format!("{:.2}%", spread);
        }
    }
    rows
}

// ====== ОБРАБОТЧИКИ ======
async fn index(State(pool): State<PgPool>) -> IndexTemplate {
    fetch_and_store(&pool).await;
    let rows = get_main_rows(&pool).await;
    IndexTemplate { rows }
}

async fn refresh_data(State(pool): State<PgPool>) -> TableTemplate {
    fetch_and_store(&pool).await;
    let rows = get_main_rows(&pool).await;
    TableTemplate { rows }
}

async fn test_telegram_handler() -> impl IntoResponse {
    let text = "🧪 <b>Тестовое сообщение</b>\nЭто проверка связи DCR Monitor и Telegram.";
    match send_telegram(text).await {
        Ok(_) => (axum::http::StatusCode::OK, axum::response::Html("<span style='color: green; font-weight: bold;'>✅ Успешно отправлено!</span>")).into_response(),
        Err(e) => (axum::http::StatusCode::OK, axum::response::Html(format!("<span style='color: red; font-weight: bold;'>❌ {}</span>", e))).into_response(),
    }
}

async fn history(State(pool): State<PgPool>, Query(query): Query<HistoryQuery>) -> HistoryTemplate {
    let period = query.period.unwrap_or_else(|| "24h".to_string());
    let now = Utc::now();
    let cutoff = match period.as_str() {
        "1h" => Some(now - Duration::hours(1)),
        "24h" => Some(now - Duration::days(1)),
        "7d" => Some(now - Duration::days(7)),
        _ => None,
    };

    let records = if let Some(cut) = cutoff {
        let cutoff_str = cut.to_rfc3339();
        sqlx::query_as::<_, HistoryRow>(
            "SELECT source, price, volume_24h, fetched_at FROM snapshots WHERE fetched_at >= $1 ORDER BY id DESC LIMIT 200"
        ).bind(cutoff_str).fetch_all(&pool).await.unwrap_or_default()
    } else {
        sqlx::query_as::<_, HistoryRow>(
            "SELECT source, price, volume_24h, fetched_at FROM snapshots ORDER BY id DESC LIMIT 200"
        ).fetch_all(&pool).await.unwrap_or_default()
    };

    let mut rows = Vec::new();
    let mut cp_points = Vec::new();
    let mut cmc_points = Vec::new();

    for r in records {
        let (date, time) = get_local_parts(&r.fetched_at);
        let x_val = format!("{}-{}-{}T{}", date.split('.').nth(2).unwrap_or(""), date.split('.').nth(1).unwrap_or(""), date.split('.').nth(0).unwrap_or(""), time);
        if r.source == "CoinPaprika" {
            cp_points.push(serde_json::json!({ "x": x_val, "y": r.price }));
        } else if r.source == "CoinMarketCap" {
            cmc_points.push(serde_json::json!({ "x": x_val, "y": r.price }));
        }
        rows.push(HistoryRowFormatted { source: r.source, price: format!("{:.2}", r.price), volume: format_volume(r.volume_24h), date, time });
    }

    cp_points.reverse();
    cmc_points.reverse();
    
    HistoryTemplate { rows, current_period: period, chart_data_cp: serde_json::to_string(&cp_points).unwrap_or_else(|_| "[]".to_string()), chart_data_cmc: serde_json::to_string(&cmc_points).unwrap_or_else(|_| "[]".to_string()) }
}

// ====== MAIN (АДАПТИРОВАННЫЙ ПОД RENDER) ======
#[tokio::main]
async fn main() {
    // Получаем строку подключения из переменных окружения Render
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:password@localhost:5432/dcr".to_string()); // fallback для локалки

    // Подключаемся к базе
    let pool = PgPool::connect(&database_url)
        .await
        .expect("Не удалось подключиться к базе данных");

    // Создаем таблицу (синтаксис Postgres: SERIAL вместо AUTOINCREMENT)
    sqlx::query("CREATE TABLE IF NOT EXISTS snapshots (id SERIAL PRIMARY KEY, source TEXT NOT NULL, price DOUBLE PRECISION, volume_24h DOUBLE PRECISION, fetched_at TEXT NOT NULL)")
        .execute(&pool).await.unwrap();

    // Запускаем фоновый планировщик
    let sched = JobScheduler::new().await.unwrap();
    let pool_clone = pool.clone();
    sched.add(
        Job::new_async("0 * * * * *", move |_uuid, _l| {
            let pool = pool_clone.clone();
            Box::pin(async move { fetch_and_store(&pool).await; })
        }).unwrap()
    ).await.unwrap();
    sched.start().await.unwrap();

    let app = Router::new()
        .route("/", get(index))
        .route("/refresh-data", get(refresh_data))
        .route("/history", get(history))
        .route("/test-telegram", post(test_telegram_handler))
        .with_state(pool);

    // Render требует слушать порт, который он передаёт через PORT
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await.unwrap();
    
    println!("Сервер запущен на порту {}", port);
    axum::serve(listener, app).await.unwrap();
}