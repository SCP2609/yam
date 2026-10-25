# Адаптер событий IPC

YAM — IPC-клиент, а не прокси и не man-in-the-middle. Поддерживаются только read-only события:

- `alarmd`: `QuasarMessage.alarm_event.alarm_fired` (поле 215);
- `aliced`: `QuasarMessage.alice_state.recognized_phrase` (поле 3) и финальные команды-спуфера (`event_source.type == "command"`).

Транспорт в SDK — TCP с little-endian 32-bit length prefix и protobuf `QuasarMessage`. Адреса в разных прошивках не стабильны, поэтому задаются оператором daemon, а не архивом пакета:

```ini
# systemd override for yamd
[Service]
Environment=YAM_ALARMD_ADDR=127.0.0.1:9912
Environment=YAM_ALICED_ADDR=127.0.0.1:9913
```

Указанные номера приведены только как пример. Перед настройкой следует сверить назначение портов с конфигурацией конкретной прошивки; YAM не сканирует порты и не делает предположений о модели.

Если переменная не задана, yamd читает `/system/vendor/quasar/quasar.cfg` (JSON) и берёт только числовое поле `<сервис>.port` → `127.0.0.1:<port>`; остальное содержимое файла (в нём есть секреты) не используется и не логируется. Переменные окружения имеют приоритет. На Yandex Station Midi `aliced` слушает порт из `quasar.cfg` (обычно 9875), порта `alarmd` там нет — `ipc.alarm` на Midi недоступен.

**Формат кадра.** Кроме прямого `QuasarMessage` (старые прошивки), принимается обёртка Midi: поле 252 (bytes) — тело сообщения, поле 253 (varint) — номер поля `QuasarMessage`, которому это тело соответствует (3 → `AliceState`, 215 → `AlarmEvent`). Такой кадр обрабатывается как `QuasarMessage{<253>: <252>}`.

**Поле `source`.** События речи содержат `source`: `"voice"` — `AliceState.recognized_phrase` (поле 2); `"command"` — команда без wake-word (спуфер: `event_source`(10).`type`(6) = `"command"`, финальный `request_state`(15), текст — поле 1). Если оба поля есть в одном кадре, выдаётся только `command`.

Не поддерживаются отправка команд, перехват аудио, токенов, VINS/directive payload и raw protobuf. Эти данные либо чувствительны, либо способны изменить состояние штатных сервисов.

## Примеры пакетов

Оба примера должны запускаться в service-режиме и используют `alice.sleep`, чтобы корректно останавливаться через YAM.

### Сработавший будильник или таймер

`plugin.toml`:

```toml
id = "alarm-observer"
name = "Alarm observer"
version = "1.0.0"
entry = "main.lua"
permissions = ["ipc.alarm", "led"]

[launch]
mode = "service"
```

`main.lua`:

```lua
while true do
  local alarm = alice.ipc.next_alarm(30_000)
  if alarm then
    -- alarm.id, alarm.alarm_type, alarm.remind_message are available.
    -- Blink the ring when a timer or alarm begins playing.
    local frame = {}
    for i = 1, device.led_count do frame[i] = "#ff7a00" end
    alice.set(frame)
    alice.sleep(250)
    alice.off()
  else
    alice.sleep(20)
  end
end
```

### Распознанная фраза

`plugin.toml`:

```toml
id = "speech-observer"
name = "Speech observer"
version = "1.0.0"
entry = "main.lua"
permissions = ["ipc.speech"]

[launch]
mode = "service"
```

`main.lua`:

```lua
while true do
  local speech = alice.ipc.next_speech(30_000)
  if speech then
    -- Do not send speech.transcript anywhere without user consent.
    -- Example: react locally to a full recognized phrase.
    if speech.transcript == "включи ночник" then
      -- local package action
    end
  else
    alice.sleep(20)
  end
end
```

`next_alarm` и `next_speech` возвращают `nil` по истечении указанного тайм-аута. Они не блокируют и не изменяют доставку исходного IPC-сообщения сервисам Яндекса.
