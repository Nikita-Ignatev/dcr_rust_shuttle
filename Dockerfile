# Этап 1: Сборка
FROM rust:1.87-slim-bookworm AS builder

# Устанавливаем утилиты и заголовки, необходимые для сборки (нужны для OpenSSL)
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Копируем Cargo.toml
COPY Cargo.toml ./

# Создаем заглушку для сборки зависимостей
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

# Устанавливаем сертификаты и библиотеку OpenSSL для запуска
RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Копируем собранный бинарник
# ВАЖНО: имя бинарника должно совпадать с name в Cargo.toml!
COPY --from=builder /app/target/release/dcr_rust_shuttle /app/dcr_monitor
COPY --from=builder /app/templates ./templates

EXPOSE 8080

CMD ["/app/dcr_monitor"]
