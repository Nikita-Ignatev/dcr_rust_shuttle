# Этап 1: Сборка
FROM rust:1.87-slim-bookworm AS builder

# Устанавливаем утилиты и заголовки для сборки (нужны для OpenSSL)
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Копируем Cargo.toml
COPY Cargo.toml ./

# Создаем заглушку для кэширования зависимостей
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

# Копируем исходный код и шаблоны
COPY src ./src
COPY templates ./templates

# ВАЖНО: Обновляем время файла (touch), чтобы Cargo гарантированно 
# пересобрал настоящий код, а не взял пустую заглушку из кэша
RUN touch src/main.rs && cargo build --release

# Копируем бинарник с правильным именем (dcr_rust)
RUN cp target/release/dcr_rust /app/myapp

# Этап 2: Запуск
FROM debian:bookworm-slim

# Устанавливаем сертификаты и библиотеку OpenSSL для запуска
RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Копируем бинарник и шаблоны
COPY --from=builder /app/myapp /app/dcr_monitor
COPY --from=builder /app/templates ./templates

EXPOSE 8080

CMD ["/app/dcr_monitor"]
