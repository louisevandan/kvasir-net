"""Tokenizer return-contract double; real tokenizer is covered by GPU scenarios."""


class Tokenizer:
    def apply_chat_template(self, messages, **kwargs):
        if kwargs.get("return_dict") is not False:
            return {"input_ids": [1, 2, 3]}
        return [1, 2, 3]
