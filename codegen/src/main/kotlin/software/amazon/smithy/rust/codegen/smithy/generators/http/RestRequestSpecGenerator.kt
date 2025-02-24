/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.generators.http

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingResolver

/**
 * [RestRequestSpecGenerator] generates a restJson1 or restXml specific `RequestSpec`. Both protocols are routed the same.
 *
 * This class has to live in the `codegen` subproject instead of in the `codegen-server` subproject because it is used
 * by the implementations of the `serverRouterRequestSpec` of the [Protocol] interface, which is used by both subprojects
 * (even though only the `codegen-server` subproject calls `serverRouterRequestSpec`).
 */
class RestRequestSpecGenerator(
    private val httpBindingResolver: HttpBindingResolver,
    private val requestSpecModule: RuntimeType,
) {
    fun generate(operationShape: OperationShape): Writable {
        val httpTrait = httpBindingResolver.httpTrait(operationShape)
        val extraCodegenScope =
            arrayOf(
                "RequestSpec",
                "UriSpec",
                "PathAndQuerySpec",
                "PathSpec",
                "QuerySpec",
                "PathSegment",
                "QuerySegment",
            ).map {
                it to requestSpecModule.member(it)
            }.toTypedArray()

        // TODO(https://github.com/awslabs/smithy-rs/issues/950): Support the `endpoint` trait.
        val pathSegmentsVec = writable {
            withBlock("vec![", "]") {
                for (segment in httpTrait.uri.segments) {
                    val variant = when {
                        segment.isGreedyLabel -> "Greedy"
                        segment.isLabel -> "Label"
                        else -> """Literal(String::from("${segment.content}"))"""
                    }
                    rustTemplate(
                        "#{PathSegment}::$variant,",
                        *extraCodegenScope,
                    )
                }
            }
        }

        val querySegmentsVec = writable {
            withBlock("vec![", "]") {
                for (queryLiteral in httpTrait.uri.queryLiterals) {
                    val variant = if (queryLiteral.value == "") {
                        """Key(String::from("${queryLiteral.key}"))"""
                    } else {
                        """KeyValue(String::from("${queryLiteral.key}"), String::from("${queryLiteral.value}"))"""
                    }
                    rustTemplate("#{QuerySegment}::$variant,", *extraCodegenScope)
                }
            }
        }

        return writable {
            rustTemplate(
                """
                #{RequestSpec}::new(
                    #{Method}::${httpTrait.method},
                    #{UriSpec}::new(
                        #{PathAndQuerySpec}::new(
                            #{PathSpec}::from_vector_unchecked(#{PathSegmentsVec:W}),
                            #{QuerySpec}::from_vector_unchecked(#{QuerySegmentsVec:W})
                        )
                    ),
                )
                """,
                *extraCodegenScope,
                "PathSegmentsVec" to pathSegmentsVec,
                "QuerySegmentsVec" to querySegmentsVec,
                "Method" to CargoDependency.Http.asType().member("Method"),
            )
        }
    }
}
