# Описание встроенных команд Picodata

## Общие сведения

Исполняемый файл `picodata` может не только запускать инстансы, но и
выступать в роли консольной утилиты. Данная утилита позволяет запускать
дополнительные и служебные команды, не участвующие в кластерном
взаимодействии.

Общий формат запуска:

```bash
picodata <command> [<params>]
```
Ниже описанные доступные команды `picodata`.

## picodata tarantool

Открывает консоль c Lua-интерпретатором, в котором можно
взаимодействовать с СУБД аналогично тому как это происходит в обычной
консоли Tarantool. Никакая логика Picodata поверх Tarantool не
выполняется, соответственно, кластер не инициализируется и подключение к
кластеру не производится. Запускается консоль Tarantool, встроенного в
Picodata (но не установленного обычного Tarantool, если такой есть в
системе).

## picodata expel

Исключает инстанс из кластера. Применяется чтобы указать кластеру, что
инстанс больше не участвует в кворуме Raft.

Полный формат:

```bash
picodata expel --instance-id <instance-id> [--cluster-id <cluster-id>] [--peer <peer>]
```

Команда подключается к `peer` через протокол _netbox_ и отдаёт ему
внутреннюю команду на исключение `instance-id` из кластера. Команда
отправляется в raft-лог, из которого затем будет применена к таблице
инстансов и установит значение `target_grade=Expelled` для заданного
инстанса. Затем через какое-то время _governor_ возьмёт в обработку этот
`target_grade`, выполнит необходимые работы по отключению инстанса и
установит ему значение `current_grade=Expelled`. Сам инстанс после этого
остановится, его процесс завершится. В дальнейшем кластер не будет
ожидать от этого инстанса участия в кворуме. Исключённый из кластера
инстанс при попытке перезапуститься будет автоматически завершаться.

Параметр `cluster-id` проверяется перед добавлением команды в raft-лог.

Параметр `peer` — это адрес любого инстанса кластера. Формат:
`[host]:port`. Может совпадать с адресом исключаемого инстанса.

Если исключаемый инстанс является текущим raft-лидером, то лидерство
переходит другому инстансу.

Обратите внимание, что исключённый инстанс нужно снять из-под контроля
супервизора.

Значение `instance-id` исключённого инстанса может быть использовано
повторно. Для этого достаточно запустить новый инстанс с тем же
`instance-id`.

### Примеры использования picodata expel

Ниже приведены типовые ситуации и подходящие для этого команды.

1. На хосте с инстансом `i4` вышел из строя жёсткий диск, данные
   инстанса утрачены, сам инстанс неработоспособен. Какой-то из
   оставшихся инстансов доступен по адресу `192.168.104.55:3301`.

    ```bash
    picodata expel --instance-id i4 --peer 192.168.104.9:3301
    ```

2. В кластере `mycluster` из 3-х инстансов, где каждый работает на своём
   физическом сервере, происходит замена одного сервера. Выключать
   инстанс нельзя, так как оставшиеся 2 узла кластера не смогут создать
   стабильный кворум. Поэтому сначала в сеть добавляется дополнительный сервер:

    ```bash
    picodata run --instance-id i4 --peer 192.168.104.1 --cluster-id mycluster
    ```

    Далее, если на сервере с инстансом i3 настроен автоматический
    перезапуск Picodata в Systemd или как-либо иначе, то его нужно
    предварительно отключить. После этого c любого из уже работающих
    серверов кластера исключается инстанс i3:

    ```bash
    picodata expel --instance-id i3 --cluster-id mycluster
    ```

    Указанная команда подключится к `127.0.0.1:3301`, который
    самостоятельно найдёт лидера кластера и отправит ему команду на
    исключение инстанса `i3`. Когда процесс `picodata` на i3 завершится
    — сервер можно выключать.