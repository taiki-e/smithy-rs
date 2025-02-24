/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This function is only used to strip prefixes from resource IDs at the time they're passed as
// input to a request. Resource IDs returned in responses may or may not include a prefix.
/// Strip the resource type prefix from resource ID return
pub fn trim_resource_id(resource_id: &mut Option<String>) {
    const PREFIXES: &[&str] = &[
        "/hostedzone/",
        "hostedzone/",
        "/change/",
        "change/",
        "/delegationset/",
        "delegationset/",
    ];

    for prefix in PREFIXES {
        if let Some(id) = resource_id
            .as_deref()
            .unwrap_or_default()
            .strip_prefix(prefix)
        {
            *resource_id = Some(id.to_string());
            return;
        }
    }
}

#[cfg(test)]
mod test {
    use crate::route53_resource_id_preprocessor::trim_resource_id;

    #[test]
    fn does_not_change_regular_zones() {
        struct OperationInput {
            resource: Option<String>,
        }

        let mut operation = OperationInput {
            resource: Some("Z0441723226OZ66S5ZCNZ".to_string()),
        };
        trim_resource_id(&mut operation.resource);
        assert_eq!(
            &operation.resource.unwrap_or_default(),
            "Z0441723226OZ66S5ZCNZ"
        );
    }

    #[test]
    fn sanitizes_prefixed_zone() {
        struct OperationInput {
            change_id: Option<String>,
        }

        let mut operation = OperationInput {
            change_id: Some("/change/Z0441723226OZ66S5ZCNZ".to_string()),
        };
        trim_resource_id(&mut operation.change_id);
        assert_eq!(
            &operation.change_id.unwrap_or_default(),
            "Z0441723226OZ66S5ZCNZ"
        );
    }

    #[test]
    fn allow_no_leading_slash() {
        struct OperationInput {
            hosted_zone: Option<String>,
        }

        let mut operation = OperationInput {
            hosted_zone: Some("hostedzone/Z0441723226OZ66S5ZCNZ".to_string()),
        };
        trim_resource_id(&mut operation.hosted_zone);
        assert_eq!(
            &operation.hosted_zone.unwrap_or_default(),
            "Z0441723226OZ66S5ZCNZ"
        );
    }
}
