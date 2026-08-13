#!/usr/bin/env python3
"""Deterministic finite control for the v2 dual-responsibility arithmetic.

This is an audit-sized D=3 semantic-style fixture, not a production backend:
it intentionally materializes its 4x4 operational kernel so every value can be
checked.  The two carrier rows have broad transport responsibility only; the
three physical rows alone own chi, coupling, mass, density, and response.

Run from the repository root:
    python3 experiments/v2_validation/dual_responsibility_probe.py
"""

from __future__ import annotations

from dataclasses import dataclass
import math


EPS_ABS = 1e-10
EPS_REL = 1e-8
ELL = 1.4
CARRIER_OFFSET_RAD = 0.1
CARRIER_RADIUS_RAD = math.pi / 2.0 + CARRIER_OFFSET_RAD

Vec3 = tuple[float, float, float]


@dataclass(frozen=True)
class Sample:
    sample_id: str
    role: str
    center: Vec3

    @property
    def is_physical(self) -> bool:
        return self.role == "physical"


def kahan_sum(values: list[float]) -> float:
    total = 0.0
    compensation = 0.0
    for value in values:
        corrected = value - compensation
        updated = total + corrected
        compensation = (updated - total) - corrected
        total = updated
    return total


def normalize(vector: Vec3) -> Vec3:
    length = math.sqrt(kahan_sum([component * component for component in vector]))
    require(length > 0.0 and math.isfinite(length), "expected a finite nonzero vector")
    return tuple(component / length for component in vector)  # type: ignore[return-value]


def dot(left: Vec3, right: Vec3) -> float:
    return kahan_sum([left[index] * right[index] for index in range(3)])


def theta(left: Vec3, right: Vec3) -> float:
    return math.acos(max(-1.0, min(1.0, dot(left, right))))


def psi(argument: float) -> float:
    if argument >= 1.0:
        return 0.0
    require(argument >= 0.0, f"kernel argument must be nonnegative, got {argument}")
    return (1.0 - argument) ** 4 * (1.0 + 4.0 * argument)


def log_psi(argument: float) -> float:
    if argument >= 1.0:
        return -math.inf
    require(argument >= 0.0, f"kernel argument must be nonnegative, got {argument}")
    return 4.0 * math.log1p(-argument) + math.log1p(4.0 * argument)


def logsumexp(values: list[float]) -> float:
    maximum = max(values)
    require(math.isfinite(maximum), "operational kernel column has no finite self pair")
    return maximum + math.log(kahan_sum([math.exp(value - maximum) for value in values]))


def tolerance(expected: float) -> float:
    return EPS_ABS + EPS_REL * max(1.0, abs(expected))


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def require_close(actual: float, expected: float, label: str) -> None:
    residual = abs(actual - expected)
    require(
        residual <= tolerance(expected),
        f"{label}: residual {residual:.17g} exceeds {tolerance(expected):.17g}",
    )


def make_operational_kernel(controls: list[Vec3]) -> list[list[float]]:
    """Return K[a][i] using the contract's column log-normalization."""

    count = len(controls)
    kernel = [[0.0 for _ in range(count)] for _ in range(count)]
    for source_index in range(count):
        logs = [log_psi(theta(target, controls[source_index]) / ELL) for target in controls]
        normalizer = logsumexp(logs)
        for target_index, value in enumerate(logs):
            kernel[target_index][source_index] = (
                0.0 if value == -math.inf else math.exp(value - normalizer)
            )
    return kernel


def main() -> None:
    # SiteId order: +x, diagonal xy, +y, -x.  The first site supplies c0;
    # c1 follows the versioned carrier gauge but is not a physical center.
    controls = [
        (1.0, 0.0, 0.0),
        normalize((1.0, 1.0, 0.0)),
        (0.0, 1.0, 0.0),
        (-1.0, 0.0, 0.0),
    ]
    site_count = len(controls)
    omega = 1.0 / site_count
    mu = [1.0] * site_count

    b = controls[0]
    t_projection = dot(controls[1], b)
    t_raw = (
        controls[1][0] - t_projection * b[0],
        controls[1][1] - t_projection * b[1],
        controls[1][2] - t_projection * b[2],
    )
    t = normalize(t_raw)
    carrier_1 = normalize(
        (
            -math.cos(CARRIER_OFFSET_RAD) * b[0] + math.sin(CARRIER_OFFSET_RAD) * t[0],
            -math.cos(CARRIER_OFFSET_RAD) * b[1] + math.sin(CARRIER_OFFSET_RAD) * t[1],
            -math.cos(CARRIER_OFFSET_RAD) * b[2] + math.sin(CARRIER_OFFSET_RAD) * t[2],
        )
    )
    require_close(abs(dot(t, b)), 0.0, "carrier gauge tangency")
    require_close(math.sqrt(dot(carrier_1, carrier_1)), 1.0, "carrier-1 norm")

    samples = [
        Sample("carrier-0", "carrier", b),
        Sample("carrier-1", "carrier", carrier_1),
        Sample("physical-z1", "physical", controls[1]),
        Sample("physical-z2", "physical", controls[2]),
        Sample("physical-z3", "physical", controls[3]),
    ]
    carrier_indices = [index for index, sample in enumerate(samples) if not sample.is_physical]
    physical_indices = [index for index, sample in enumerate(samples) if sample.is_physical]
    require(len(carrier_indices) == 2 and len(physical_indices) == 3, "fixture role count changed")

    kernel = make_operational_kernel(controls)
    for source_index in range(site_count):
        require_close(
            kahan_sum([kernel[target_index][source_index] for target_index in range(site_count)]),
            1.0,
            f"K column {source_index}",
        )
        require(
            all(
                kernel[target_index][source_index] >= 0.0
                and math.isfinite(kernel[target_index][source_index])
                for target_index in range(site_count)
            ),
            "K is negative or non-finite",
        )

    # r/p are the only finite operational density truth for this fixture.
    r = [
        kahan_sum([mu[source_index] * kernel[target_index][source_index] for source_index in range(site_count)])
        for target_index in range(site_count)
    ]
    p = [r[target_index] / (site_count * omega) for target_index in range(site_count)]
    require(all(value > 0.0 and math.isfinite(value) for value in p), "p must be positive and finite")
    require_close(kahan_sum([omega * value for value in p]), 1.0, "p normalization")

    # phi covers all samples.  A carrier's broad radius is unrelated to its
    # physical capability, so it remains in this numerator and denominator.
    phi = [[0.0 for _ in range(site_count)] for _ in samples]
    for target_index, control in enumerate(controls):
        profiles = []
        for sample in samples:
            radius = CARRIER_RADIUS_RAD if sample.role == "carrier" else min(math.pi, 2.0 * ELL)
            profiles.append(psi(theta(control, sample.center) / radius))
        denominator = kahan_sum(profiles)
        require(denominator > 0.0 and math.isfinite(denominator), "transport phi has a zero denominator")
        for sample_index, profile in enumerate(profiles):
            phi[sample_index][target_index] = profile / denominator
        require_close(
            kahan_sum([phi[sample_index][target_index] for sample_index in range(len(samples))]),
            1.0,
            f"phi column {target_index}",
        )
    transport_volume = [kahan_sum([omega * phi[sample_index][a] for a in range(site_count)]) for sample_index in range(len(samples))]
    require_close(kahan_sum(transport_volume), 1.0, "transport volume total")
    for sample_index, volume in enumerate(transport_volume):
        require(volume > 0.0, f"{samples[sample_index].sample_id} lost transport volume")
    for sample_index in carrier_indices:
        require(transport_volume[sample_index] > 0.0, f"{samples[sample_index].sample_id} lost transport volume")

    # chi deliberately has no carrier branch.  Every semantic control must
    # have a physical denominator; zero/fallback responsibilities are forbidden.
    chi = [[0.0 for _ in range(site_count)] for _ in samples]
    for target_index, control in enumerate(controls):
        profiles = {
            sample_index: psi(theta(control, samples[sample_index].center) / (2.0 * ELL))
            for sample_index in physical_indices
        }
        denominator = kahan_sum([profiles[sample_index] for sample_index in physical_indices])
        require(denominator > 0.0 and math.isfinite(denominator), "physical chi has a zero denominator")
        for sample_index in physical_indices:
            chi[sample_index][target_index] = profiles[sample_index] / denominator
        require_close(
            kahan_sum([chi[sample_index][target_index] for sample_index in physical_indices]),
            1.0,
            f"chi column {target_index}",
        )
    for sample_index in carrier_indices:
        require(all(value == 0.0 for value in chi[sample_index]), f"{samples[sample_index].sample_id} has chi")

    physical_volume = [kahan_sum([omega * chi[sample_index][a] for a in range(site_count)]) for sample_index in range(len(samples))]
    coupling = [[0.0 for _ in range(site_count)] for _ in samples]
    for sample_index in physical_indices:
        for source_index in range(site_count):
            coupling[sample_index][source_index] = mu[source_index] * kahan_sum(
                [chi[sample_index][target_index] * kernel[target_index][source_index] for target_index in range(site_count)]
            )
        require(
            all(value >= 0.0 and math.isfinite(value) for value in coupling[sample_index]),
            f"{samples[sample_index].sample_id} has invalid coupling",
        )
    mass = [kahan_sum(row) for row in coupling]
    density = [0.0 for _ in samples]
    for sample_index in physical_indices:
        require(physical_volume[sample_index] > 0.0, f"{samples[sample_index].sample_id} has no physical volume")
        require(mass[sample_index] > 0.0, f"{samples[sample_index].sample_id} has no mass")
        density[sample_index] = mass[sample_index] / physical_volume[sample_index]
        require(density[sample_index] >= 0.0 and math.isfinite(density[sample_index]), "invalid physical density")
        require_close(
            mass[sample_index],
            kahan_sum([chi[sample_index][target_index] * r[target_index] for target_index in range(site_count)]),
            f"mass density identity for {samples[sample_index].sample_id}",
        )

    for source_index in range(site_count):
        require_close(
            kahan_sum([coupling[sample_index][source_index] for sample_index in physical_indices]),
            mu[source_index],
            f"coupling column {source_index}",
        )
    for sample_index in physical_indices:
        require_close(
            kahan_sum(coupling[sample_index]), mass[sample_index], f"coupling row {sample_index}")
    require_close(kahan_sum(mass), float(site_count), "total physical mass")

    # This scalar response probe is only the section 8.2 physical-row gate;
    # it intentionally does not stand in for the section 7 FVM transport.
    response = [0.0 for _ in samples]
    for sample_index in physical_indices:
        response[sample_index] = mass[sample_index] / site_count
        require(response[sample_index] > 0.0, f"{samples[sample_index].sample_id} has no response")
    scattered_response = [
        kahan_sum(
            [
                (coupling[sample_index][source_index] / mass[sample_index]) * response[sample_index]
                for sample_index in physical_indices
            ]
        )
        for source_index in range(site_count)
    ]
    for source_index in range(site_count):
        require_close(scattered_response[source_index], mu[source_index] / site_count, f"response scatter {source_index}")

    p_hat = [
        kahan_sum([density[sample_index] * chi[sample_index][target_index] for sample_index in physical_indices]) / site_count
        for target_index in range(site_count)
    ]
    for target_index in range(site_count):
        carrier_density = [
            density[sample_index] * chi[sample_index][target_index] / site_count
            for sample_index in carrier_indices
        ]
        require(all(value == 0.0 for value in carrier_density), "carrier density leaked into p_hat")
    require_close(kahan_sum([omega * value for value in p_hat]), 1.0, "p_hat normalization")
    etv = 0.5 * kahan_sum([omega * abs(p[target_index] - p_hat[target_index]) for target_index in range(site_count)])
    require(math.isfinite(etv) and 0.0 <= etv <= 1.0, "E_TV must be finite and bounded")

    for sample_index in carrier_indices:
        sample = samples[sample_index]
        require(physical_volume[sample_index] == 0.0, f"{sample.sample_id} has physical volume")
        require(all(value == 0.0 for value in coupling[sample_index]), f"{sample.sample_id} has coupling")
        require(mass[sample_index] == 0.0, f"{sample.sample_id} has mass")
        require(density[sample_index] == 0.0, f"{sample.sample_id} has density")
        require(response[sample_index] == 0.0, f"{sample.sample_id} has response")

    print("dual-responsibility finite control: PASS")
    print(f"K column sums: {[round(kahan_sum([kernel[a][i] for a in range(site_count)]), 12) for i in range(site_count)]}")
    print(f"carrier V^T: {[round(transport_volume[index], 12) for index in carrier_indices]}")
    print(f"physical mass: {[round(mass[index], 12) for index in physical_indices]}")
    print(f"E_TV_op: {etv:.12g}")


if __name__ == "__main__":
    main()
