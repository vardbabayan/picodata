from conftest import Postgres
import psycopg
from decimal import Decimal
from uuid import UUID
import pg8000.native as pg8000  # type: ignore


def test_decimal(postgres: Postgres):
    user = "postgres"
    password = "P@ssw0rd"
    host = postgres.host
    port = postgres.port

    # create a postgres user using a postgres compatible password
    postgres.instance.sql(
        f"CREATE USER \"{user}\" WITH PASSWORD '{password}' USING md5"
    )
    # allow user to create tables
    postgres.instance.sudo_sql(f'GRANT CREATE TABLE TO "{user}"')

    # connect to the server and enable autocommit as we
    # don't support interactive transactions
    conn = psycopg.connect(
        f"user = {user} password={password} host={host} port={port} sslmode=disable"
    )
    conn.autocommit = True

    conn.execute(
        """
        CREATE TABLE T (
            W_ID DECIMAL NOT NULL,
            PRIMARY KEY (W_ID)
        )
        USING MEMTX DISTRIBUTED BY (W_ID);
        """
    )

    pi = Decimal("3.1415926535897932384626433832")
    e = Decimal("2.7182818284590452353602874713")
    nines = Decimal("999999999999999999999999999")
    ones = nines / 9

    # test text decoding
    conn.execute(
        """
        INSERT INTO T VALUES (%t), (%t)
        """,
        (pi, nines),
    )

    # test binary decoding
    conn.execute(
        """
        INSERT INTO T VALUES (%b), (%b)
        """,
        (e, ones),
    )

    # test text encoding
    cur = conn.execute(
        """
        SELECT * FROM T
        """,
        binary=False,
    )
    assert sorted(cur.fetchall()) == [
        (e,),
        (pi,),
        (ones,),
        (nines,),
    ]

    # test binary encoding
    cur = conn.execute(
        """
        SELECT * FROM T
        """,
        binary=True,
    )
    assert sorted(cur.fetchall()) == [
        (e,),
        (pi,),
        (ones,),
        (nines,),
    ]


def test_uuid(postgres: Postgres):
    user = "postgres"
    password = "P@ssw0rd"
    host = postgres.host
    port = postgres.port

    # create a postgres user using a postgres compatible password
    postgres.instance.sql(
        f"CREATE USER \"{user}\" WITH PASSWORD '{password}' USING md5"
    )
    # allow user to create tables
    postgres.instance.sudo_sql(f'GRANT CREATE TABLE TO "{user}"')

    # connect to the server and enable autocommit as we
    # don't support interactive transactions
    conn = psycopg.connect(
        f"user = {user} password={password} host={host} port={port} sslmode=disable"
    )
    conn.autocommit = True

    conn.execute(
        """
        CREATE TABLE T (
            ID UUID NOT NULL,
            PRIMARY KEY (ID)
        )
        USING MEMTX DISTRIBUTED BY (ID);
        """
    )

    id1 = UUID("6f2ba4c4-0a4c-4d79-86ae-43d4f84b70e1")
    id2 = UUID("e4166fc5-e113-46c5-8ae9-970882ca8842")

    # test text decoding
    conn.execute(""" INSERT INTO T VALUES(%t); """, (id1,))

    # test binary decoding
    conn.execute(""" INSERT INTO T VALUES(%b); """, (id2,))

    # test text encoding
    cur = conn.execute(""" SELECT * FROM T; """, binary=False)
    assert sorted(cur.fetchall()) == [(id1,), (id2,)]

    # test binary encoding
    cur = conn.execute(""" SELECT * FROM T; """, binary=True)
    assert sorted(cur.fetchall()) == [(id1,), (id2,)]


def test_text_and_varchar(postgres: Postgres):
    user = "postgres"
    password = "P@ssw0rd"

    postgres.instance.sql(
        f"CREATE USER \"{user}\" WITH PASSWORD '{password}' USING md5"
    )
    postgres.instance.sudo_sql(f'GRANT CREATE TABLE TO "{user}"')

    conn = pg8000.Connection(
        user, password=password, host=postgres.host, port=postgres.port
    )

    conn.run(
        """
        CREATE TABLE T (
            S TEXT NOT NULL,
            PRIMARY KEY (S)
        )
        USING MEMTX DISTRIBUTED BY (S);
        """
    )

    # encode string parameter as varchar
    varchar_oid = 1043
    conn.run("INSERT INTO T VALUES (:p);", p="value1", types={"p": varchar_oid})

    # encode string parameter as text
    text_oid = 25
    conn.run("INSERT INTO T VALUES (:p);", p="value2", types={"p": text_oid})

    # verify that the values were insert
    rows = conn.run("SELECT * FROM T;")
    assert rows == [["value1"], ["value2"]]