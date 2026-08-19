impl SequencePayload {
    fn encode_body(&self, limits: ProtocolLimits, output: &mut Vec<u8>) -> Result<(), FrameError> {
        if self.descriptors.len() != self.payloads.len() {
            return Err(FrameError::InvalidSequence("descriptor/payload count"));
        }
        if self.descriptors.len() > limits.max_descriptors {
            return Err(FrameError::TooManyDescriptors);
        }
        if self.n_tokens == Some(0) {
            return Err(FrameError::InvalidSequence("token count"));
        }
        let sequence_id = self.sequence_id.as_bytes();
        if sequence_id.len() > limits.max_name_bytes {
            return Err(FrameError::NameTooLong);
        }
        put_u32(
            output,
            u32::try_from(sequence_id.len()).map_err(|_| FrameError::NameTooLong)?,
        );
        output.extend_from_slice(sequence_id);
        put_u32(
            output,
            u32::try_from(self.descriptors.len()).map_err(|_| FrameError::TooManyDescriptors)?,
        );
        let mut payload_total = 0usize;
        for (index, (descriptor, payload)) in self
            .descriptors
            .iter()
            .zip(self.payloads.iter())
            .enumerate()
        {
            descriptor.validate(limits)?;
            validate_alias(index, descriptor, self.descriptors.len())?;
            encode_descriptor(output, descriptor, limits)?;
            if descriptor.alias_of.is_none() {
                let bytes = payload
                    .as_deref()
                    .ok_or(FrameError::InvalidSequence("missing descriptor payload"))?;
                if bytes.len() != usize::try_from(descriptor.nbytes).unwrap_or(usize::MAX) {
                    return Err(FrameError::PayloadLengthMismatch {
                        declared: descriptor.nbytes,
                        actual: bytes.len() as u64,
                    });
                }
                payload_total = payload_total
                    .checked_add(bytes.len())
                    .ok_or(FrameError::PayloadTooLarge)?;
                if payload_total > limits.max_payload_bytes {
                    return Err(FrameError::PayloadTooLarge);
                }
                put_u64(output, bytes.len() as u64);
                output.extend_from_slice(bytes);
            } else if payload.is_some() {
                return Err(FrameError::InvalidSequence("alias payload must be omitted"));
            }
        }
        if let Some(n_tokens) = self.n_tokens {
            output.extend_from_slice(&TOKEN_COUNT_MAGIC);
            put_u32(output, n_tokens);
        }
        Ok(())
    }

    pub fn encode(&self, limits: ProtocolLimits) -> Result<Vec<u8>, FrameError> {
        let mut output = Vec::new();
        self.encode_body(limits, &mut output)?;
        if output.len() > limits.max_frame_bytes {
            return Err(FrameError::FrameTooLarge);
        }
        Ok(output)
    }

    fn encode_v2(&self, limits: ProtocolLimits) -> Result<Vec<u8>, FrameError> {
        let mut output = Vec::new();
        let has_prompt = self.prompt.is_some();
        let has_tokens = self.initial_tokens.is_some();
        let has_outcome = self.outcome.is_some();
        let has_position = self.position.is_some();
        let has_options = !self.options.is_empty();
        if has_options {
            validate_text(&self.options, limits.max_frame_bytes)?;
        }
        output.push(
            (has_prompt as u8)
                | ((has_tokens as u8) << 1)
                | ((has_outcome as u8) << 2)
                | ((has_position as u8) << 3)
                | ((has_options as u8) << 4),
        );
        if let Some(prompt) = &self.prompt {
            validate_text(prompt, limits.max_frame_bytes)?;
            put_u32(
                &mut output,
                u32::try_from(prompt.len()).map_err(|_| FrameError::FrameTooLarge)?,
            );
            output.extend_from_slice(prompt.as_bytes());
        }
        if let Some(tokens) = &self.initial_tokens {
            if tokens.len() > limits.max_descriptors {
                return Err(FrameError::TooManyDescriptors);
            }
            put_u32(
                &mut output,
                u32::try_from(tokens.len()).map_err(|_| FrameError::TooManyDescriptors)?,
            );
            for token in tokens {
                output.extend_from_slice(&token.to_le_bytes());
            }
        }
        if let Some(position) = self.position {
            put_u32(&mut output, position);
        }
        if let Some(outcome) = &self.outcome {
            validate_text(&outcome.text, limits.max_frame_bytes)?;


            if let Some(stop) = &outcome.stop {
                validate_text(stop, limits.max_name_bytes)?;
            }
            put_u32(&mut output, outcome.token as u32);
            put_u32(&mut output, outcome.position);
            put_u32(
                &mut output,
                u32::try_from(outcome.text.len()).map_err(|_| FrameError::FrameTooLarge)?,
            );
            output.extend_from_slice(outcome.text.as_bytes());
            output.push(u8::from(outcome.stop.is_some()));
            if let Some(stop) = &outcome.stop {
                put_u32(
                    &mut output,
                    u32::try_from(stop.len()).map_err(|_| FrameError::NameTooLong)?,
                );
                output.extend_from_slice(stop.as_bytes());
            }
        }
        if has_options {
            put_u32(
                &mut output,
                u32::try_from(self.options.len()).map_err(|_| FrameError::FrameTooLarge)?,
            );
            output.extend_from_slice(self.options.as_bytes());
        }
        self.encode_body(limits, &mut output)?;
        if output.len() > limits.max_frame_bytes {
            return Err(FrameError::FrameTooLarge);
        }
        Ok(output)
    }

    fn decode_body(bytes: &[u8], limits: ProtocolLimits) -> Result<Self, FrameError> {
        let mut cursor = Cursor::new(bytes);
        let sequence_id_len =
            usize::try_from(cursor.u32()?).map_err(|_| FrameError::NameTooLong)?;
        if sequence_id_len > limits.max_name_bytes {
            return Err(FrameError::NameTooLong);
        }
        let sequence_id = String::from_utf8(cursor.bytes(sequence_id_len)?.to_vec())
            .map_err(|_| FrameError::InvalidSequence("sequence id utf-8"))?;
        let count = usize::try_from(cursor.u32()?).map_err(|_| FrameError::TooManyDescriptors)?;
        if count > limits.max_descriptors {
            return Err(FrameError::TooManyDescriptors);
        }
        let mut descriptors = Vec::with_capacity(count);
        let mut payloads = Vec::with_capacity(count);
        let mut payload_total = 0usize;
        for index in 0..count {
            let descriptor = decode_descriptor(&mut cursor, limits)?;
            descriptor.validate(limits)?;
            validate_alias(index, &descriptor, count)?;
            if descriptor.alias_of.is_some() {
                descriptors.push(descriptor);
                payloads.push(None);
                continue;
            }
            let payload_len =
                usize::try_from(cursor.u64()?).map_err(|_| FrameError::PayloadTooLarge)?;
            if payload_len > limits.max_payload_bytes {
                return Err(FrameError::PayloadTooLarge);
            }
            let payload = cursor.bytes(payload_len)?.to_vec();
            if payload.len() as u64 != descriptor.nbytes {
                return Err(FrameError::PayloadLengthMismatch {
                    declared: descriptor.nbytes,
                    actual: payload.len() as u64,
                });
            }
            payload_total = payload_total
                .checked_add(payload.len())
                .ok_or(FrameError::PayloadTooLarge)?;
            if payload_total > limits.max_payload_bytes {
                return Err(FrameError::PayloadTooLarge);
            }
            descriptors.push(descriptor);
            payloads.push(Some(payload));
        }
        let n_tokens = if cursor.remaining() == TOKEN_COUNT_MAGIC.len() + 4
            && cursor.bytes(TOKEN_COUNT_MAGIC.len())? == TOKEN_COUNT_MAGIC
        {
            Some(cursor.u32()?)
        } else {
            None
        };
        if n_tokens == Some(0) {
            return Err(FrameError::InvalidSequence("token count"));
        }
        if cursor.remaining() != 0 {
            return Err(FrameError::InvalidSequence("trailing sequence metadata"));
        }
        Ok(Self {
            sequence_id,
            descriptors,
            payloads,
            n_tokens,
            prompt: None,
            initial_tokens: None,
            position: None,
            options: String::new(),
            outcome: None,
        })
    }

    fn decode_v2(bytes: &[u8], limits: ProtocolLimits) -> Result<Self, FrameError> {
        if bytes.is_empty() {
            return Err(FrameError::Truncated);
        }
        let mut cursor = Cursor::new(bytes);
        let flags = cursor.u8()?;
        if flags & !0x1f != 0 {
            return Err(FrameError::InvalidSequence("reserved HOP sequence flags"));
        }
        let prompt = if flags & 1 != 0 {
            let len = usize::try_from(cursor.u32()?).map_err(|_| FrameError::FrameTooLarge)?;
            if len > limits.max_frame_bytes {
                return Err(FrameError::FrameTooLarge);
            }
            Some(
                String::from_utf8(cursor.bytes(len)?.to_vec())
                    .map_err(|_| FrameError::InvalidSequence("prompt utf-8"))?,
            )
        } else {
            None
        };
        let initial_tokens = if flags & 2 != 0 {
            let count =
                usize::try_from(cursor.u32()?).map_err(|_| FrameError::TooManyDescriptors)?;
            if count > limits.max_descriptors {
                return Err(FrameError::TooManyDescriptors);
            }
            let mut tokens = Vec::with_capacity(count);
            for _ in 0..count {
                tokens.push(i32::from_le_bytes(cursor.bytes(4)?.try_into().unwrap()));
            }
            Some(tokens)
        } else {
            None
        };
        let position = if flags & 8 != 0 {
            Some(cursor.u32()?)
        } else {
            None
        };
        let outcome = if flags & 4 != 0 {
            let token = cursor.u32()? as i32;
            let position = cursor.u32()?;
            let text_len = usize::try_from(cursor.u32()?).map_err(|_| FrameError::FrameTooLarge)?;
            if text_len > limits.max_frame_bytes {
                return Err(FrameError::FrameTooLarge);
            }
            let text = String::from_utf8(cursor.bytes(text_len)?.to_vec())
                .map_err(|_| FrameError::InvalidSequence("outcome text utf-8"))?;
            let has_stop = cursor.u8()?;
            if has_stop > 1 {
                return Err(FrameError::InvalidSequence("outcome stop flag"));
            }
            let stop = if has_stop == 1 {
                let stop_len =
                    usize::try_from(cursor.u32()?).map_err(|_| FrameError::NameTooLong)?;
                if stop_len > limits.max_name_bytes {
                    return Err(FrameError::NameTooLong);
                }
                Some(
                    String::from_utf8(cursor.bytes(stop_len)?.to_vec())
                        .map_err(|_| FrameError::InvalidSequence("outcome stop utf-8"))?,
                )
            } else {
                None
            };
            Some(OutcomeMetadata {
                token,
                text,
                position,
                stop,
            })
        } else {
            None
        };
        let options = if flags & 16 != 0 {
            let len = usize::try_from(cursor.u32()?).map_err(|_| FrameError::FrameTooLarge)?;
            if len > limits.max_frame_bytes {
                return Err(FrameError::FrameTooLarge);
            }
            Some(
                String::from_utf8(cursor.bytes(len)?.to_vec())
                    .map_err(|_| FrameError::InvalidSequence("options utf-8"))?,
            )
        } else {
            None
        };
        let mut result = Self::decode_body(cursor.bytes(cursor.remaining())?, limits)?;
        result.prompt = prompt;
        result.initial_tokens = initial_tokens;
        result.position = position;
        result.options = options.unwrap_or_default();
        result.outcome = outcome;
        Ok(result)
    }

    pub fn decode(bytes: &[u8], limits: ProtocolLimits) -> Result<Self, FrameError> {
        if bytes.len() > limits.max_frame_bytes {
            return Err(FrameError::FrameTooLarge);
        }
        Self::decode_body(bytes, limits)
    }
}
