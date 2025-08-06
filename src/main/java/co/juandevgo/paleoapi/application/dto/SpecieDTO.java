package co.juandevgo.paleoapi.application.dto;

import com.fasterxml.jackson.annotation.JsonProperty;
import jakarta.validation.constraints.NotBlank;
import jakarta.validation.constraints.Pattern;
import lombok.AllArgsConstructor;
import lombok.Builder;
import lombok.Data;
import lombok.NoArgsConstructor;

import java.util.List;

@Data
@AllArgsConstructor
@NoArgsConstructor
@Builder
public class SpecieDTO {

    @JsonProperty
    @NotBlank
    @Pattern(regexp = "^[a-zA-Z\\s]+$")
    private String name;

    @JsonProperty
    @NotBlank
    @Pattern(regexp = "^[a-zA-Z\\s]+$")
    private String group;

    @JsonProperty
    @Pattern(regexp = "^[a-zA-Z\\s-]+$")
    private String phonetic;

    @JsonProperty
    @NotBlank
    @Pattern(regexp = "^[a-zA-Z\\s,]+$")
    private String diet;

    @JsonProperty
    @NotBlank
    @Pattern(regexp = "^[a-zA-Z0-9\\s-.,]+$")
    private String size;

    @JsonProperty
    @NotBlank
    @Pattern(regexp = "^[a-zA-Z0-9\\s-,.]+$")
    private String timePeriod;

    @JsonProperty
    private List<String> locations;

    @JsonProperty
    private List<String> classification;

    @JsonProperty
    private List<String> images;
}