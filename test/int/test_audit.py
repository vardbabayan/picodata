import pytest
import json

from dataclasses import dataclass
from typing import Optional, ClassVar
from enum import Enum

from tarantool.error import (  # type: ignore
    NetworkError,
)

from conftest import (
    Instance,
    Cluster,
    retrying,
)


class Severity(str, Enum):
    Low = "low"
    Medium = "medium"
    High = "high"


@dataclass
class Event:
    id: str
    time: str
    title: str
    message: str
    severity: Severity

    @staticmethod
    def parse(s):
        match s["title"]:
            case EventLocalStartup.TITLE:
                return EventLocalStartup(**s)
            case EventLocalShutdown.TITLE:
                return EventLocalShutdown(**s)
            case EventInitAudit.TITLE:
                return EventInitAudit(**s)
            case EventAuthOk.TITLE:
                return EventAuthOk(**s)
            case EventAuthFail.TITLE:
                return EventAuthFail(**s)
            case EventChangeConfig.TITLE:
                return EventChangeConfig(**s)
            case EventChangePassword.TITLE:
                return EventChangePassword(**s)
            case EventChangeTargetGrade.TITLE:
                return EventChangeTargetGrade(**s)
            case EventChangeCurrentGrade.TITLE:
                return EventChangeCurrentGrade(**s)
            case EventJoinInstance.TITLE:
                return EventJoinInstance(**s)
            case EventExpelInstance.TITLE:
                return EventExpelInstance(**s)
            case EventGrantPrivilege.TITLE:
                return EventGrantPrivilege(**s)
            case EventRevokePrivilege.TITLE:
                return EventRevokePrivilege(**s)
            case EventGrantRole.TITLE:
                return EventGrantRole(**s)
            case EventRevokeRole.TITLE:
                return EventRevokeRole(**s)
            case EventCreateRole.TITLE:
                return EventCreateRole(**s)
            case EventDropRole.TITLE:
                return EventDropRole(**s)
            case EventCreateUser.TITLE:
                return EventCreateUser(**s)
            case EventDropUser.TITLE:
                return EventDropUser(**s)
            case EventCreateTable.TITLE:
                return EventCreateTable(**s)
            case EventDropTable.TITLE:
                return EventDropTable(**s)


@dataclass
class EventLocalStartup(Event):
    TITLE: ClassVar[str] = "local_startup"


@dataclass
class EventLocalShutdown(Event):
    TITLE: ClassVar[str] = "local_shutdown"


@dataclass
class EventInitAudit(Event):
    TITLE: ClassVar[str] = "init_audit"


@dataclass
class EventAuthOk(Event):
    TITLE: ClassVar[str] = "auth_ok"
    user: str


@dataclass
class EventAuthFail(Event):
    TITLE: ClassVar[str] = "auth_fail"
    user: str
    verdict: Optional[str] = None


@dataclass
class EventChangeConfig(Event):
    TITLE: ClassVar[str] = "change_config"
    key: str
    value: Optional[str] = None


@dataclass
class EventChangePassword(Event):
    TITLE: ClassVar[str] = "change_password"
    user: str
    auth_type: str


@dataclass
class EventChangeTargetGrade(Event):
    TITLE: ClassVar[str] = "change_target_grade"
    instance_id: str
    raft_id: str
    new_grade: str


@dataclass
class EventChangeCurrentGrade(Event):
    TITLE: ClassVar[str] = "change_current_grade"
    instance_id: str
    raft_id: str
    new_grade: str


@dataclass
class EventJoinInstance(Event):
    TITLE: ClassVar[str] = "join_instance"
    instance_id: str
    raft_id: str


@dataclass
class EventExpelInstance(Event):
    TITLE: ClassVar[str] = "expel_instance"
    instance_id: str
    raft_id: str


@dataclass
class EventGrantPrivilege(Event):
    TITLE: ClassVar[str] = "grant_privilege"
    privilege: str
    object: str
    object_type: str
    grantee: str
    grantee_type: str


@dataclass
class EventRevokePrivilege(Event):
    TITLE: ClassVar[str] = "revoke_privilege"
    privilege: str
    object: str
    object_type: str
    grantee: str
    grantee_type: str


@dataclass
class EventGrantRole(Event):
    TITLE: ClassVar[str] = "grant_role"
    role: str
    grantee: str
    grantee_type: str


@dataclass
class EventRevokeRole(Event):
    TITLE: ClassVar[str] = "revoke_role"
    role: str
    grantee: str
    grantee_type: str


@dataclass
class EventCreateRole(Event):
    TITLE: ClassVar[str] = "create_role"
    role: str


@dataclass
class EventDropRole(Event):
    TITLE: ClassVar[str] = "drop_role"
    role: str


@dataclass
class EventCreateUser(Event):
    TITLE: ClassVar[str] = "create_user"
    user: str
    auth_type: str


@dataclass
class EventDropUser(Event):
    TITLE: ClassVar[str] = "drop_user"
    user: str


@dataclass
class EventCreateTable(Event):
    TITLE: ClassVar[str] = "create_table"
    name: str


@dataclass
class EventDropTable(Event):
    TITLE: ClassVar[str] = "drop_table"
    name: str


class AuditFile:
    def __init__(self, path):
        self._f = open(path)

    def events(self):
        for line in self._f:
            yield Event.parse(json.loads(line))


def take_until_type(events, event_class: type):
    for event in events:
        if isinstance(event, event_class):
            return event
    return None


def test_startup(instance: Instance):
    instance.start()
    instance.terminate()

    events = list(AuditFile(instance.audit_flag_value).events())
    assert len(events) > 0

    # Check identifiers
    i = 1
    for event in events:
        assert event.id == f"1.0.{i}"
        i += 1

    # These should be the first two events
    assert isinstance(events[0], EventInitAudit)
    assert events[0].message == "audit log is ready"
    assert events[0].severity == Severity.Low
    assert isinstance(events[1], EventLocalStartup)
    assert events[1].message == "instance is starting"
    assert events[1].severity == Severity.Low

    event = take_until_type(iter(events), EventJoinInstance)
    assert event is not None
    assert event.instance_id == "i1"
    assert event.raft_id == "1"

    event = take_until_type(iter(events), EventChangeTargetGrade)
    assert event is not None
    assert event.new_grade == "Offline(0)"
    assert event.instance_id == "i1"
    assert event.raft_id == "1"
    assert (
        event.message
        == f"target grade of instance `{event.instance_id}` changed to {event.new_grade}"
    )
    assert event.severity == Severity.Low

    event = take_until_type(iter(events), EventChangeCurrentGrade)
    assert event is not None
    assert event.new_grade == "Offline(0)"
    assert event.instance_id == "i1"
    assert event.raft_id == "1"
    assert (
        event.message
        == f"current grade of instance `{event.instance_id}` changed to {event.new_grade}"
    )
    assert event.severity == Severity.Medium

    event = take_until_type(iter(events), EventChangeConfig)
    assert event is not None


def test_create_drop_table(instance: Instance):
    instance.start()
    instance.sql(
        """
        create table "foo" ("val" int not null, primary key ("val"))
        distributed by ("val")
        """
    )
    instance.sql(
        """
        drop table "foo"
        """
    )
    instance.terminate()

    events = AuditFile(instance.audit_flag_value).events()

    create_table = take_until_type(events, EventCreateTable)
    assert create_table is not None
    assert create_table.name == "foo"
    assert create_table.message == "created table `foo`"
    assert create_table.severity == Severity.Medium

    drop_table = take_until_type(events, EventDropTable)
    assert drop_table is not None
    assert drop_table.name == "foo"
    assert drop_table.message == "dropped table `foo`"
    assert drop_table.severity == Severity.Medium


def test_user(instance: Instance):
    instance.start()
    instance.sql(
        """
        create user "ymir" with password '0123456789' using chap-sha1
        """
    )
    instance.sql(
        """
        alter user "ymir" password '9876543210'
        """
    )
    instance.sql(
        """
        drop user "ymir"
        """
    )
    instance.terminate()

    events = AuditFile(instance.audit_flag_value).events()

    create_user = take_until_type(events, EventCreateUser)
    assert create_user is not None
    assert create_user.user == "ymir"
    assert create_user.auth_type == "chap-sha1"
    assert create_user.message == f"created user `{create_user.user}`"
    assert create_user.severity == Severity.High

    change_password = take_until_type(events, EventChangePassword)
    assert change_password is not None
    assert change_password.user == "ymir"
    assert change_password.auth_type == "chap-sha1"
    assert (
        change_password.message
        == f"password of user `{change_password.user}` was changed"
    )
    assert change_password.severity == Severity.High

    drop_user = take_until_type(events, EventDropUser)
    assert drop_user is not None
    assert drop_user.user == "ymir"
    assert drop_user.message == f"dropped user `{drop_user.user}`"
    assert drop_user.severity == Severity.Medium


def test_role(instance: Instance):
    instance.start()
    instance.sql(
        """
        create role "skibidi"
        """
    )
    instance.sql(
        """
        create role "dummy"
        """
    )
    instance.sudo_sql(
        """
        grant "dummy" to "skibidi"
        """
    )
    instance.sudo_sql(
        """
        revoke "dummy" from "skibidi"
        """
    )
    instance.sql(
        """
        drop role "skibidi"
        """
    )
    instance.terminate()

    events = AuditFile(instance.audit_flag_value).events()

    create_role = take_until_type(events, EventCreateRole)
    assert create_role is not None
    assert create_role.role == "skibidi"
    assert create_role.message == f"created role `{create_role.role}`"
    assert create_role.severity == Severity.High

    grant_role = take_until_type(events, EventGrantRole)
    assert grant_role is not None
    # assert grant_role.role == "dummy"  # FIXME: currently it's u32
    assert grant_role.grantee == "skibidi"
    assert grant_role.grantee_type == "role"
    assert (
        grant_role.message
        == f"granted role `{grant_role.role}` to role `{grant_role.grantee}`"
    )
    assert grant_role.severity == Severity.High

    revoke_role = take_until_type(events, EventRevokeRole)
    assert revoke_role is not None
    # assert revoke_role.role == "dummy"  # FIXME: currently it's u32
    assert revoke_role.grantee == "skibidi"
    assert revoke_role.grantee_type == "role"
    assert (
        revoke_role.message
        == f"revoked role `{grant_role.role}` from role `{revoke_role.grantee}`"
    )
    assert revoke_role.severity == Severity.High

    drop_role = take_until_type(events, EventDropRole)
    assert drop_role is not None
    assert drop_role.role == "skibidi"
    assert drop_role.message == f"dropped role `{drop_role.role}`"
    assert drop_role.severity == Severity.Medium


def assert_instance_expelled(expelled_instance: Instance, instance: Instance):
    info = instance.call("pico.instance_info", expelled_instance.instance_id)
    grades = (info["current_grade"]["variant"], info["target_grade"]["variant"])
    assert grades == ("Expelled", "Expelled")


def test_join_expel_instance(cluster: Cluster):
    cluster.deploy(instance_count=1)
    i1 = cluster.instances[0]

    audit_i1 = AuditFile(i1.audit_flag_value)
    for _ in audit_i1.events():
        pass
    events = audit_i1.events()

    i2 = cluster.add_instance(instance_id="i2")

    join_instance = take_until_type(events, EventJoinInstance)
    assert join_instance is not None
    assert join_instance.instance_id == "i2"
    assert join_instance.raft_id == str(i2.raft_id)
    assert join_instance.severity == Severity.Low

    cluster.expel(i2)
    retrying(lambda: assert_instance_expelled(i2, i1))

    expel_instance = take_until_type(events, EventExpelInstance)
    assert expel_instance is not None
    assert expel_instance.instance_id == "i2"
    assert expel_instance.raft_id == str(i2.raft_id)
    assert expel_instance.severity == Severity.Low


def test_auth(instance: Instance):
    instance.start()

    instance.sudo_sql(
        """
        create user "ymir" with password '0123456789' using chap-sha1
        """
    )
    instance.sudo_sql(
        """
        alter user "ymir" login
        """
    )

    audit = AuditFile(instance.audit_flag_value)
    for _ in audit.events():
        pass
    events = audit.events()

    with instance.connect(4, user="ymir", password="0123456789") as _:
        pass

    auth_ok = take_until_type(events, EventAuthOk)
    assert auth_ok is not None
    assert auth_ok.user == "ymir"
    assert auth_ok.severity == Severity.High

    with pytest.raises(NetworkError):
        with instance.connect(4, user="ymir", password="wrong_pwd") as _:
            pass

    auth_fail = take_until_type(events, EventAuthFail)
    assert auth_fail is not None
    assert auth_fail.user == "ymir"
    assert auth_fail.severity == Severity.High