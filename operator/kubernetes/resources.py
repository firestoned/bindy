"""
Kubernetes resource creation and management
"""

from kubernetes import client
import kubernetes
from operator.kubernetes.client import get_k8s_client
import logging

logger = logging.getLogger(__name__)


def create_bind9_statefulset(spec, name, namespace, uid, logger):
    """Create BIND9 StatefulSet"""

    k8s = get_k8s_client()

    replicas = spec.get("replicas", 1)
    image = spec.get("version", "internetsystemsconsortium/bind9:9.18")

    statefulset = client.V1StatefulSet(
        api_version="apps/v1",
        kind="StatefulSet",
        metadata=client.V1ObjectMeta(
            name=f"{name}-bind9",
            namespace=namespace,
            owner_references=[
                client.V1OwnerReference(
                    api_version="dns.example.com/v1alpha1",
                    kind="Bind9Instance",
                    name=name,
                    uid=uid,
                    controller=True,
                    block_owner_deletion=True,
                )
            ],
        ),
        spec=client.V1StatefulSetSpec(
            service_name=f"{name}-headless",
            replicas=replicas,
            selector=client.V1LabelSelector(match_labels={"app": f"{name}-bind9"}),
            template=client.V1PodTemplateSpec(
                metadata=client.V1ObjectMeta(labels={"app": f"{name}-bind9"}),
                spec=client.V1PodSpec(
                    containers=[
                        client.V1Container(
                            name="bind9",
                            image=image,
                            ports=[
                                client.V1ContainerPort(
                                    container_port=53, protocol="UDP", name="dns-udp"
                                ),
                                client.V1ContainerPort(
                                    container_port=53, protocol="TCP", name="dns-tcp"
                                ),
                            ],
                            volume_mounts=[
                                client.V1VolumeMount(
                                    name="config", mount_path="/etc/bind", read_only=True
                                ),
                                client.V1VolumeMount(
                                    name="zones", mount_path="/etc/bind/zones", read_only=True
                                ),
                            ],
                        )
                    ],
                    volumes=[
                        client.V1Volume(
                            name="config",
                            config_map=client.V1ConfigMapVolumeSource(name=f"{name}-config"),
                        ),
                        client.V1Volume(
                            name="zones",
                            config_map=client.V1ConfigMapVolumeSource(name=f"{name}-zones"),
                        ),
                    ],
                ),
            ),
        ),
    )

    try:
        k8s["apps"].create_namespaced_stateful_set(namespace=namespace, body=statefulset)
        logger.info(f"StatefulSet {name}-bind9 created")
    except kubernetes.client.exceptions.ApiException as e:
        if e.status == 409:
            k8s["apps"].patch_namespaced_stateful_set(
                name=f"{name}-bind9", namespace=namespace, body=statefulset
            )


def create_bind9_services(name, namespace, uid):
    """Create services for BIND9"""

    k8s = get_k8s_client()

    service = client.V1Service(
        api_version="v1",
        kind="Service",
        metadata=client.V1ObjectMeta(
            name=f"{name}-dns",
            namespace=namespace,
            owner_references=[
                client.V1OwnerReference(
                    api_version="dns.example.com/v1alpha1", kind="Bind9Instance", name=name, uid=uid
                )
            ],
        ),
        spec=client.V1ServiceSpec(
            selector={"app": f"{name}-bind9"},
            ports=[
                client.V1ServicePort(port=53, protocol="UDP", name="dns-udp"),
                client.V1ServicePort(port=53, protocol="TCP", name="dns-tcp"),
            ],
            type="LoadBalancer",
        ),
    )

    try:
        k8s["core"].create_namespaced_service(namespace=namespace, body=service)
    except kubernetes.client.exceptions.ApiException as e:
        if e.status != 409:
            raise


def create_configmap(name, namespace, data, uid, cm_type="config"):
    """Create ConfigMap"""

    k8s = get_k8s_client()

    cm_name = f"{name}-{cm_type}"

    configmap = client.V1ConfigMap(
        api_version="v1",
        kind="ConfigMap",
        metadata=client.V1ObjectMeta(
            name=cm_name,
            namespace=namespace,
            owner_references=[
                client.V1OwnerReference(
                    api_version="dns.example.com/v1alpha1", kind="Bind9Instance", name=name, uid=uid
                )
            ],
        ),
        data={f"{cm_type}.conf": data},
    )

    try:
        k8s["core"].create_namespaced_config_map(namespace=namespace, body=configmap)
    except kubernetes.client.exceptions.ApiException as e:
        if e.status == 409:
            k8s["core"].patch_namespaced_config_map(
                name=cm_name, namespace=namespace, body=configmap
            )


def reload_bind9_config(instance_name, namespace, logger):
    """Reload BIND9 configuration"""
    logger.info(f"Reloading BIND9 config for {instance_name}")
    # Implementation would use kubectl exec or similar


def update_zone_file(zone_ref, namespace, record_name, record_type, spec, logger):
    """Update zone file with new record"""
    logger.info(f"Updating zone {zone_ref} with {record_type} record {record_name}")
    # Implementation would update the zone ConfigMap
