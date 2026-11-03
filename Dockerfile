# Этап 1: Сборка
FROM rust:1.87-slim-bookworm AS builder

# Устанавливаем утилиты и заголовки для сборки (нужны для OpenSSL)
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

# Находим собранный файл, даем ему имя 'myapp' и кладем в корень /app
# Это избавит нас от необходимости угадывать имя из Cargo.toml
RUN cp target/release/$(ls target/release/ | grep -v -E '^(build|deps|examples|incremental)$' | grep -v '\.d$' | head -n 1) /app/myapp

# Этап 2: Запуск
FROM debian:bookworm-slim

# Устанавливаем сертификаты и библиотеку OpenSSL для запуска
RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Копируем наш переименованный бинарник
COPY --from=builder /app/myapp /app/dcr_monitor
# Копируем шаблоны
COPY --from=builder /app/templates ./templates

EXPOSE 8080

CMD ["/app/dcr_monitor"]
