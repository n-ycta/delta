# Restore access to an existing Delta node

## «Раньше был browser-owner, ключа никогда не было»

`browser-owner` — временный доступ к узлу, а не отдельная учётная запись с паролем. Истечение этой сессии само по себе не удаляет плагины, настройки или данные. Новый ключ, созданный в браузере, ещё не зарегистрирован на узле; попытка входа им даёт `public key is not enrolled`.

**Не выполняйте `delta init --force`, не удаляйте `DELTA_HOME` и не переустанавливайте узел ради входа.** Восстановление выполняется на прежнем компьютере/VPS под тем же системным пользователем. Оно требует уже существующий локальный `admin.token`, не создаёт новый узел и не открывает анонимный доступ.

### Linux / VPS

Скачайте [recover-web.mjs](../scripts/recover-web.mjs), проверьте исходник и запустите его встроенным Node:

```sh
"${DELTA_HOME:-$HOME/.delta}/runtime/bin/node" ./recover-web.mjs
```

Если Node установлен отдельно, можно выполнить `node ./recover-web.mjs`. Для нестандартного расположения укажите `--home /путь/к/прежнему/узлу`. Если Web запущен не штатным пакетом, укажите также `--web-data /абсолютный/путь/к/прежним/данным/web`. Не подставляйте новую пустую директорию.

Команда сверяет ID запущенного локального ядра с прежним `config.json`, проверяет права владельца и записывает только одноразовую ссылку в существующее хранилище Web. Существующие плагины, ключи, сессии, настройки и данные не изменяются.

На VPS ссылка по умолчанию локальная. На своём компьютере откройте SSH-туннель к **тому же серверу и пользователю**:

```sh
ssh -L 7710:127.0.0.1:7710 USER@HOST
```

После этого откройте выданную ссылку локально в браузере в течение пяти минут. Если Web использует другой порт, замените порт в туннеле на фактический. Для опубликованной HTTPS Console можно указать `--origin https://console.example`; этот адрес должен быть настроен в `PUBLIC_ORIGIN` Web и `identity_origins` ядра. Команда не меняет доверенные адреса автоматически.

### Windows

В PowerShell под исходным пользователем:

```powershell
& "$env:LOCALAPPDATA\Delta\runtime\node.exe" .\recover-web.mjs
```

При нестандартном расположении добавьте `--home 'C:\путь\к\прежнему\Delta'`.

### После открытия ссылки

1. Откройте «Ключи и восстановление» на прежнем узле.
2. Создайте и сохраните новый ключ **либо импортируйте ранее созданный, но ещё не зарегистрированный ключ**.
3. Нажмите «Зарегистрировать». Сохранение фразы и регистрация — разные действия.
4. Дождитесь успешного входа с ключом. Тогда временный токен владельца отзывается.

В старых версиях Console импорт ещё не показан: после восстановления можно создать новый ключ, сохранить фразу и нажать «Зарегистрировать публичный ключ». Не выходите из временной сессии до регистрации. После успешного входа проверьте старые плагины и настройки.

Не присылайте никому `admin.token`, `config.json`, одноразовую ссылку, приватный ключ или seed-фразу. Если фраза попала на опубликованный скриншот, после восстановления зарегистрируйте новую, проверьте её вход и только потом отзовите старую.

## If recovery stops

- **Files not found:** use the original OS user and original `DELTA_HOME`. The command will not initialise a replacement node.
- **Node ID mismatch:** another node is listening on that port. Stop and check the old service configuration; do not reset either profile.
- **Owner credential rejected:** restore the existing administrative credential from a private backup, or use an already enrolled owner key. There is no public owner-registration bypass.
- **Core offline:** start the original service; recovery does not install or start a replacement node.
- **Public key not enrolled:** creating a key is not enrollment. Recover the existing node first, then register the key there.
- **Origin not allowed:** use an allowed localhost tunnel, or explicitly configure the verified HTTPS address on the same node.

## Security boundaries

The tool makes only authenticated `core.info` and `identity.list` calls over loopback and creates a five-minute, hashed, one-use bootstrap file with private permissions. It does not print the existing admin token or modify `config.json`. The link must be exchanged over HTTPS or localhost with an exact matching Origin. Anonymous browsers cannot issue a recovery link or enroll themselves as owner.


## Existing installation: update without the Console

`node scripts/update-existing.mjs --recover --open` runs as the original OS user and uses the existing private owner credential. It updates only installed, enabled official plugins, pins the catalog version/checksum, waits for package jobs, preserves the node identity and peers, saves a private configuration backup, then issues a fresh five-minute owner recovery link. Disabled and third-party plugins are not silently enabled or replaced. It does not update the Core executable or native APK/EXE hosts. Core 0.4.4 is the current published stable Core; preview source changes are not installed by this repair command. No database snapshot or multi-package transactional rollback is implied.

Run on the original PC, not as root and not in a new Delta home. Never run `delta init --force`. Save a new recovery key and explicitly register it on the recovered node.
