# Этап 1: Сборка
FROM rust:1.75-slim AS builder

WORKDIR /app

# Копируем манифесты для кэширования зависимостей
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

# Копируем исходный код и шаблоны
COPY src ./src
COPY templates ./templates

# Собираем финальный бинарник
RUN cargo build --release

# Этап 2: Запуск
FROM debian:bookworm-slim

# Устанавливаем корневые сертификаты (нужны для HTTPS-запросов к API)
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Копируем собранный бинарник из первого этапа
COPY --from=builder /app/target/release/dcr_rust /app/dcr_monitor
# Копируем шаблоны (они нужны программе для работы)
COPY --from=builder /app/templates ./templates

# Открываем порт (RelaxDev обычно использует 8080)
EXPOSE 8080

# Запускаем приложение
CMD ["/app/dcr_monitor"]