"""Transformation checks independent of the full model and speech fixtures."""
import importlib.util
from pathlib import Path
import unittest
import numpy as np
import onnx
from onnx import helper as h, numpy_helper as nh, TensorProto as T
import onnxruntime as ort

spec = importlib.util.spec_from_file_location("simplify", Path(__file__).with_name("simplify-pianissimo.py"))
simplify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(simplify)


def make_model(nodes):
    model = h.make_model(h.make_graph(nodes, "test", [h.make_tensor_value_info("x", T.FLOAT, [2])],
                                    [h.make_tensor_value_info("out", T.FLOAT, [2])]),
                         opset_imports=[h.make_opsetid("", 18)])
    model.ir_version = 9
    return model


class TestCommonExpressions(unittest.TestCase):
    def test_merges_named_constants_and_cascading_expressions(self):
        nodes = [h.make_node("Constant", [], [name], value=nh.from_array(np.array([1,2], dtype=np.float32), name))
                 for name in ("a", "b")]
        nodes += [h.make_node("Add", ["x", "a"], ["xa"]), h.make_node("Add", ["x", "b"], ["xb"]),
                  h.make_node("Mul", ["xa", "xb"], ["out"])]
        model = make_model(nodes)
        original = model.SerializeToString()
        removed = simplify.merge_expressions(model)
        self.assertEqual(removed, {"Constant": 1, "Add": 1})
        onnx.checker.check_model(model)
        for x in (np.array([0,1],dtype=np.float32), np.array([-7,9],dtype=np.float32)):
            expected = ort.InferenceSession(original).run(None, {"x":x})[0]
            actual = ort.InferenceSession(model.SerializeToString()).run(None, {"x":x})[0]
            np.testing.assert_array_equal(actual, expected)

    def test_preserves_graph_output_and_distinct_values(self):
        nodes = [h.make_node("Constant", [], [name], value=nh.from_array(np.array([n,n],dtype=np.float32)))
                 for name,n in (("a",1),("b",2))]
        nodes += [h.make_node("Add", ["x","a"], ["first"]), h.make_node("Add", ["x","a"], ["out"])]
        model = make_model(nodes)
        self.assertEqual(simplify.merge_expressions(model), {})
        self.assertEqual(model.graph.node[-1].output[0], "out")

    def test_does_not_merge_random_ops(self):
        model = make_model([h.make_node("RandomNormal", [], [name], shape=[2]) for name in ("a","b","out")])
        self.assertEqual(simplify.merge_expressions(model), {})

    def test_rejects_subgraphs(self):
        branch = h.make_graph([], "branch", [], [])
        model = make_model([h.make_node("If", ["condition"], ["out"], then_branch=branch, else_branch=branch)])
        with self.assertRaisesRegex(ValueError, "Subgraphs"):
            simplify.merge_expressions(model)


if __name__ == "__main__":
    unittest.main()
